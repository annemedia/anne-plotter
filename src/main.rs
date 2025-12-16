mod cpu_hasher;
#[cfg(feature = "opencl")]
mod gpu_hasher;
#[cfg(feature = "opencl")]
mod ocl;
mod plotter;
mod poc_hashing;
mod scheduler;
mod shabal256;
mod utils;
mod writer;
mod buffer;

use std::cmp::min;
use std::process;

use clap::{Arg, ArgAction, ArgGroup, Command};
use crate::plotter::{Plotter, PlotterTask};
use crate::utils::set_low_prio;

fn main() {
    let mut cmd = Command::new("anne-plotter")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg_required_else_help(true)
        // Removed display_order_derive — clap 4 handles ordering well by default
        .arg(
            Arg::new("disable_direct_io")
                .short('d')
                .long("ddio")
                .help("Disables direct i/o")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("disable_async_io")
                .short('a')
                .long("daio")
                .help("Disables async writing (single RAM buffer mode)")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("low_priority")
                .short('l')
                .long("prio")
                .help("Runs with low priority")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .help("Runs in non-verbose mode")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("benchmark")
                .short('b')
                .long("bench")
                .help("Runs in xPU benchmark mode")
                .action(ArgAction::SetTrue)
                .global(true),
        )
        .arg(
            Arg::new("numeric_id")
                .short('i')
                .long("id")
                .value_name("NUMERIC_ID")
                .help("Your numeric Account ID")
                .value_parser(clap::value_parser!(u64))
                .required_unless_present("ocl_devices"),
        )
        .arg(
            Arg::new("start_nonce")
                .short('s')
                .long("sn")
                .value_name("START_NONCE")
                .help("Starting nonce for plotting")
                .value_parser(clap::value_parser!(u64))
                // Required unless either start_nonce_auto or ocl_devices is present
                .required_unless_present("start_nonce_auto")
                .required_unless_present("ocl_devices"),
        )
        .arg(
            Arg::new("start_nonce_auto")
                .short('A')
                .long("sna")
                .value_name("COUNT")
                .help("Auto-plot COUNT (>=1) sequential files, each with --n nonces, starting after the last existing plot for this ID. Ignores --sn.")
                .value_parser(clap::value_parser!(u64))
                .conflicts_with("start_nonce"),
        )
        .arg(
            Arg::new("nonces")
                .short('n')
                .long("n")
                .value_name("NONCES")
                .help("How many nonces you want to plot")
                .value_parser(clap::value_parser!(u64))
                .required_unless_present("ocl_devices"),
        )
        .arg(
            Arg::new("path")
                .short('p')
                .long("path")
                .value_name("PATH")
                .help("Target path for plotfile (optional)"),
        )
        .arg(
            Arg::new("memory")
                .short('m')
                .long("mem")
                .value_name("MEMORY")
                .help("Maximum memory usage (optional)")
                .default_value("0B"),
        )
        .arg(
            Arg::new("cpu")
                .short('c')
                .long("cpu")
                .value_name("THREADS")
                .help("Maximum cpu cores you want to use (optional)")
                .value_parser(clap::value_parser!(u8)),
        )
        .arg(
            Arg::new("gpu")
                .short('g')
                .long("gpu")
                .value_name("platform_id:device_id:cores")
                .help("GPU(s) you want to use for plotting (optional)")
                .action(ArgAction::Append),
        )
        .group(
            ArgGroup::new("processing")
                .args(["cpu", "gpu"])
                .multiple(true),
        );

    #[cfg(feature = "opencl")]
    {
        cmd = cmd
            .arg(
                Arg::new("ocl_devices")
                    .short('o')
                    .long("opencl")
                    .help("Display OpenCL platforms and devices")
                    .action(ArgAction::SetTrue)
                    .global(true),
            )
            .arg(
                Arg::new("zero_copy")
                    .short('z')
                    .long("zcb")
                    .help("Enables zero copy buffers for shared mem (integrated) gpus")
                    .action(ArgAction::SetTrue)
                    .global(true),
            );
    }

    let matches = cmd.get_matches();

    if matches.get_flag("low_priority") {
        set_low_prio();
    }

    #[cfg(feature = "opencl")]
    if matches.get_flag("ocl_devices") {
        ocl::platform_info();
        return;
    }

    let numeric_id = *matches.get_one::<u64>("numeric_id").expect("numeric_id required");

    let nonces = *matches.get_one::<u64>("nonces").expect("nonces required");

    let output_path = matches
        .get_one::<String>("path")
        .cloned()
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap()
                .into_os_string()
                .into_string()
                .unwrap()
        });

    let mem = matches.get_one::<String>("memory").cloned().unwrap();

    let cpu_threads_input = matches.get_one::<u8>("cpu").copied().unwrap_or(0);

    // Fixed type inference
    let gpus: Option<Vec<String>> = matches
        .get_many::<String>("gpu")
        .map(|v| v.cloned().collect());

    // CPU thread calculation
    let cores = sys_info::cpu_num().unwrap() as u8;
    let mut cpu_threads = if cpu_threads_input == 0 {
        cores
    } else {
        min(2 * cores, cpu_threads_input)
    };

    #[cfg(feature = "opencl")]
    if matches.contains_id("gpu") && !matches.contains_id("cpu") {
        cpu_threads = 0;
    }

    let p = Plotter::new();

    if let Some(&auto_count) = matches.get_one::<u64>("start_nonce_auto") {
        if auto_count == 0 {
            eprintln!("Error: --sna count must be >= 1");
            process::exit(1);
        }

        if !matches.get_flag("quiet") {
            println!("--sna enabled: plotting {auto_count} sequential file(s)");
        }

        let mut current_start = 0u64;
        let mut actual_nonces_per_file: u64 = nonces;  // fallback

        // First: scan for existing files to find where to continue
        if let Ok(entries) = std::fs::read_dir(&output_path) {
            let mut max_end: u64 = 0;
            let prefix = format!("{}_", numeric_id);

            for entry in entries.flatten() {
                if let Some(file_name) = entry.file_name().to_str() {
                    if file_name.starts_with(&prefix) {
                        let parts: Vec<&str> = file_name.split('_').collect();
                        if parts.len() >= 3 {
                            if let (Ok(sn), Ok(cnt)) = (parts[1].parse::<u64>(), parts[2].parse::<u64>()) {
                                let end = sn + cnt;
                                if end > max_end {
                                    max_end = end;
                                    actual_nonces_per_file = cnt;  // Use real plotted count
                                }
                            }
                        }
                    }
                }
            }
            current_start = max_end;
        }

        if !matches.get_flag("quiet") {
            println!("Starting from nonce {current_start}");
            if actual_nonces_per_file != nonces {
                println!("Detected aligned nonce count per file: {actual_nonces_per_file} (from existing file)");
            }
        }

        // Now plot each file one by one
        for i in 0..auto_count {
            let this_start = current_start + i * actual_nonces_per_file;

            if !matches.get_flag("quiet") {
                println!("\n--- Plotting file {} of {auto_count}: start_nonce = {this_start} ---", i + 1);
            }

            let file_task = PlotterTask {
                numeric_id,
                start_nonce: this_start,
                nonces,  // pass original — plotter will round it down if needed
                output_path: output_path.clone(),
                mem: mem.clone(),
                cpu_threads,
                gpus: gpus.clone(),
                direct_io: !matches.get_flag("disable_direct_io"),
                async_io: !matches.get_flag("disable_async_io"),
                quiet: matches.get_flag("quiet"),
                benchmark: matches.get_flag("benchmark"),
                zcb: matches.get_flag("zero_copy"),
            };

            p.run(file_task);

            // After plotting this file, update actual_nonces_per_file from the new filename
            // (in case alignment changed or first file)
            if let Ok(entries) = std::fs::read_dir(&output_path) {
                let mut latest_cnt: u64 = actual_nonces_per_file;
                let prefix = format!("{}_", numeric_id);

                for entry in entries.flatten() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.starts_with(&prefix) {
                            let parts: Vec<&str> = file_name.split('_').collect();
                            if parts.len() >= 3 {
                                if let (Ok(sn), Ok(cnt)) = (parts[1].parse::<u64>(), parts[2].parse::<u64>()) {
                                    if sn == this_start {
                                        latest_cnt = cnt;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                actual_nonces_per_file = latest_cnt;
            }
        }
    } else {
        let start_nonce = *matches.get_one::<u64>("start_nonce").expect("--sn is required when not using --sna");

        p.run(PlotterTask {
            numeric_id,
            start_nonce,
            nonces,
            output_path,
            mem,
            cpu_threads,
            gpus,
            direct_io: !matches.get_flag("disable_direct_io"),
            async_io: !matches.get_flag("disable_async_io"),
            quiet: matches.get_flag("quiet"),
            benchmark: matches.get_flag("benchmark"),
            zcb: matches.get_flag("zero_copy"),
        });
    }
}