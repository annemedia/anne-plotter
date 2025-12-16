use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

cfg_if::cfg_if! {
    if #[cfg(unix)] {
        #[cfg(target_os = "linux")]
        extern crate thread_priority;
        use std::process::Command;
        use std::process;
        use std::os::unix::fs::OpenOptionsExt;
        use fs2::FileExt;
        #[cfg(target_os = "linux")]
        use thread_priority::*;

        const O_DIRECT: i32 = 0o0_040_000;

        pub fn set_low_prio() {

            #[cfg(target_os = "linux")]
            set_current_thread_priority(ThreadPriority::Min).unwrap();
        }

        pub fn open_using_direct_io<P: AsRef<Path>>(path: P) -> io::Result<File> {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .custom_flags(O_DIRECT)
                .open(path)
        }

        pub fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
            OpenOptions::new()
                .write(true)
                .create(true)
                .open(path)
        }

        pub fn open_r<P: AsRef<Path>>(path: P) -> io::Result<File> {
            OpenOptions::new()
                .read(true)
                .open(path)
        }
        

        fn get_device_id_unix(path: &str) -> String {

            let path_obj = Path::new(path);
            let parent = path_obj.parent()
                .unwrap_or_else(|| Path::new("/"));
            
            if !parent.exists() {
                panic!("Parent directory does not exist: {:?}. Please create it first.", parent);
            }
            

            let actual_path = parent.to_str().unwrap();
            

            let output = Command::new("df")
                .arg("--output=source")
                .arg(actual_path)
                .output()
                .expect("failed to execute 'df --output=source'");
            
            let source = String::from_utf8(output.stdout).expect("not utf8");
            let lines: Vec<&str> = source.trim().split('\n').collect();
            
            if lines.len() >= 2 {
                let device = lines[1].trim();
                if !device.is_empty() {
                    return device.to_string();
                }
            }
            
            panic!("Could not determine device for path: {} (parent: {})", path, actual_path);
        }

        fn get_sector_size_macos(path: &str) -> u64 {
            let source = get_device_id_unix(path);
            let output = Command::new("diskutil")
                .arg("info")
                .arg(&source)
                .output()
                .expect("failed to execute 'diskutil info'");
            let source = String::from_utf8(output.stdout).expect("not utf8");
            let mut sector_size: u64 = 0;
            for line in source.split('\n') {
                if line.trim().starts_with("Device Block Size") {

                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() >= 2 {
                        let value_part = parts[1].trim();

                        let size_str = value_part.split_whitespace().next().unwrap_or("4096");
                        sector_size = size_str.parse::<u64>().unwrap_or(4096);
                        break;
                    }
                }
            }
            if sector_size == 0 {
                panic!("Abort: Unable to determine disk physical sector size from diskutil info")
            }
            sector_size
        }

        fn get_sector_size_unix(path: &str) -> u64 {
            let source = get_device_id_unix(path);
            

            let output = match Command::new("lsblk")
                .arg(&source)
                .arg("-o")
                .arg("PHY-SEC")
                .arg("-b")
                .arg("-n")
                .output() {
                    Ok(output) => output,
                    Err(_) => {

                        return get_sector_size_fallback(&source);
                    }
                };

            let sector_size_str = String::from_utf8(output.stdout).expect("not utf8");
            let sector_size = sector_size_str.trim();
            
            if sector_size.is_empty() {
                return get_sector_size_fallback(&source);
            }
            
            sector_size.parse::<u64>().unwrap_or_else(|_| {
                println!("Warning: Failed to parse sector size '{}', defaulting to 512", sector_size);
                4096
            })
        }

        fn get_sector_size_fallback(device: &str) -> u64 {
            match Command::new("blockdev")
                .arg("--getpbsz")
                .arg(device)
                .output() {
                    Ok(output) => {
                        let size_str = String::from_utf8(output.stdout).expect("not utf8");
                        size_str.trim().parse::<u64>().unwrap_or(4096)
                    }
                    Err(_) => {
                        println!("Warning: Could not determine sector size, defaulting to 4096");
                        4096
                    }
                }
        }

        pub fn get_sector_size(path: &str) -> u64 {
            if cfg!(target_os = "macos") {
                get_sector_size_macos(path)
            } else {
                get_sector_size_unix(path)
            }
        }

        pub fn preallocate(file: &Path, size_in_bytes: u64, use_direct_io: bool) {
            if use_direct_io {

                preallocate_direct_io(file, size_in_bytes)
            } else {
                preallocate_normal(file, size_in_bytes)
            }
        }

        fn preallocate_normal(file: &Path, size_in_bytes: u64) {
            let file = open(&file).unwrap();
            match file.allocate(size_in_bytes) {
                Err(errno) => {
                    eprintln!("\n\nError: couldn't preallocate space for file. {}\n\
                            Probable causes are:\n \
                            * fallocate() is only supported on ext4 filesystems.\n \
                            * Insufficient space.\n", errno);
                    process::exit(1);
                }
                Ok(_) => (),
            }
        }

        fn preallocate_direct_io(file: &Path, size_in_bytes: u64) {

            let sector_size = get_sector_size(file.to_str().unwrap_or("/"));
            let aligned_size = ((size_in_bytes + sector_size - 1) / sector_size) * sector_size;
            

            let file_result = open_using_direct_io(&file);
            
            match file_result {
                Ok(file) => {

                    use std::os::unix::io::AsRawFd;
                    use libc::{ftruncate, c_int};
                    
                    let fd = file.as_raw_fd();
                    
                    unsafe {
                        if ftruncate(fd as c_int, aligned_size as i64) != 0 {
                            let err = io::Error::last_os_error();
                            println!("\n\nError: couldn't allocate space for O_DIRECT file. {}\n\
                                      Probable causes are:\n \
                                      * O_DIRECT requires aligned sizes\n \
                                      * Filesystem doesn't support O_DIRECT properly\n \
                                      * Try running without direct I/O flag\n", err);
                            process::exit(1);
                        }
                    }
                }
                Err(e) => {

                    eprintln!("\nWarning: O_DIRECT open failed: {}. Using normal I/O.", e);
                    preallocate_normal(file, size_in_bytes);
                }
            }
        }

        pub fn free_disk_space(path: &str) -> u64 {

            fs2::available_space(Path::new(&path)).unwrap().saturating_sub(2097152)
        }

    } else {
        use std::ffi::CString;
        use std::ptr::null_mut;
        use std::iter::once;
        use std::ffi::OsStr;
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::OpenOptionsExt;
        use core::mem::size_of_val;
        use winapi::um::errhandlingapi::GetLastError;
        use winapi::um::fileapi::{GetDiskFreeSpaceA,SetFileValidData};
        use winapi::um::handleapi::CloseHandle;
        use winapi::um::processthreadsapi::{SetThreadIdealProcessor,GetCurrentThread,OpenProcessToken,GetCurrentProcess,SetPriorityClass};
        use winapi::um::securitybaseapi::AdjustTokenPrivileges;
        use winapi::um::winbase::LookupPrivilegeValueW;
        use winapi::um::winnt::{LUID,TOKEN_ADJUST_PRIVILEGES,TOKEN_PRIVILEGES,LUID_AND_ATTRIBUTES,SE_PRIVILEGE_ENABLED,SE_MANAGE_VOLUME_NAME};

        const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
        const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

        pub fn open_using_direct_io<P: AsRef<Path>>(path: P) -> io::Result<File> {
            OpenOptions::new()
                .write(true)
                .create(true)
                .custom_flags(FILE_FLAG_NO_BUFFERING)
                .open(path)
        }

        pub fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
            OpenOptions::new()
                .write(true)
                .create(true)
                .custom_flags(FILE_FLAG_WRITE_THROUGH)
                .open(path)
        }

        pub fn open_r<P: AsRef<Path>>(path: P) -> io::Result<File> {
            OpenOptions::new()
                .read(true)
                .open(path)
        }

        pub fn preallocate(file: &Path, size_in_bytes: u64, use_direct_io: bool) {
            let mut result = true;
            result &= obtain_priviledge();

            let file = if use_direct_io {
                open_using_direct_io(&file)
            } else {
                open(&file)
            };
            let file = file.unwrap();

            file.set_len(size_in_bytes).unwrap();

            if result {
                let handle = file.as_raw_handle();
                unsafe{
                    let temp = SetFileValidData(handle, size_in_bytes as i64);
                    result &= temp == 1;
                }
            }

            if !result {
                println!("FAILED, administrative rights missing");
                print!("Slow file pre-allocation...");
            }
        }

        pub fn obtain_priviledge() -> bool {
            let mut result = true;

            let privilege_encoded: Vec<u16> = OsStr::new(SE_MANAGE_VOLUME_NAME)
                .encode_wide()
                .chain(once(0))
                .collect();

            let luid = LUID{
                HighPart: 0i32,
                LowPart: 0u32

            };

            unsafe {
                let mut htoken = null_mut();
                let mut tp = TOKEN_PRIVILEGES{
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES{
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                    }]
                };

                let temp = OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut htoken);
                 result &= temp == 1;

                let temp = LookupPrivilegeValueW(null_mut(), privilege_encoded.as_ptr(), &mut tp.Privileges[0].Luid);
                result &= temp == 1;

                let temp = AdjustTokenPrivileges(htoken, 0, &mut tp, size_of_val(&tp) as u32, null_mut(), null_mut());

                CloseHandle(htoken);
                result &= temp == 1;
                result &=
                    GetLastError() == 0u32
            }
            result
        }

        pub fn get_sector_size(path: &str) -> u64 {
            let path_encoded = Path::new(path);
            let parent_path_encoded = CString::new(path_encoded.to_str().unwrap()).unwrap();
            let mut sectors_per_cluster  = 0u32;
            let mut bytes_per_sector  = 0u32;
            let mut number_of_free_cluster  = 0u32;
            let mut total_number_of_cluster  = 0u32;
            if unsafe {
                GetDiskFreeSpaceA(
                    parent_path_encoded.as_ptr(),
                    &mut sectors_per_cluster,
                    &mut bytes_per_sector,
                    &mut number_of_free_cluster,
                    &mut total_number_of_cluster
                )
            } == 0  {
                panic!("get sector size, filename={}",path);
            };
            u64::from(bytes_per_sector)
        }

        pub fn set_thread_ideal_processor(id: usize){

        unsafe {
            SetThreadIdealProcessor(
                GetCurrentThread(),
                id as u32
            );
            }
        }
        pub fn set_low_prio() {
            unsafe{
                SetPriorityClass(GetCurrentProcess(),BELOW_NORMAL_PRIORITY_CLASS);
            }
        }
        pub fn free_disk_space(path: &str) -> u64 {
            fs2::available_space(Path::new(&path)).unwrap()
        }
    }
}
