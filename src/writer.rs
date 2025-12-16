use crate::plotter::{PlotterTask, NONCE_SIZE, SCOOP_SIZE};
use crate::buffer::PageAlignedByteBuffer;
use crate::utils::{open, open_r, open_using_direct_io};
use crossbeam_channel::{Receiver, Sender};
use std::cmp::min;
use std::io::{Read, Seek, SeekFrom, Write, Error, ErrorKind};
use std::path::Path;
use std::sync::Arc;
use indicatif::ProgressBar;

const TASK_SIZE: u64 = 16384;

pub fn create_writer_thread(
    task: Arc<PlotterTask>,
    mut nonces_written: u64,
    pb: Option<ProgressBar>,
    rx_buffers_to_writer: Receiver<PageAlignedByteBuffer>,
    tx_empty_buffers: Sender<PageAlignedByteBuffer>,
) -> impl FnOnce() {
    move || {
        for buffer in rx_buffers_to_writer {
            let mut_bs = &buffer.get_buffer();
            let bs = mut_bs.lock().unwrap();
            let buffer_size = (*bs).len() as u64;
            let nonces_to_write = min(buffer_size / NONCE_SIZE, task.nonces - nonces_written);

            let filename = Path::new(&task.output_path).join(format!(
                "{}_{}_{}",
                task.numeric_id, task.start_nonce, task.nonces
            ));
            if !task.benchmark {
                let file_result = if task.direct_io {
                    open_using_direct_io(&filename)
                } else {
                    open(&filename)
                };
                let mut file = match file_result {
                    Ok(f) => f,
                    Err(e) if e.raw_os_error() == Some(libc::EINVAL as i32) => {
                      //  eprintln!("Warning: O_DIRECT open failed: {}. Using normal I/O.", e);
                        match open(&filename) {
                            Ok(f) => f,
                            Err(e2) => {
                                eprintln!("Error: Normal open also failed: {}", e2);

                                tx_empty_buffers.send(buffer).unwrap();
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: File open failed: {}", e);

                        tx_empty_buffers.send(buffer).unwrap();
                        continue;
                    }
                };

                for scoop in 0..4096 {
                    let mut seek_addr = scoop * task.nonces as u64 * SCOOP_SIZE;
                    seek_addr += nonces_written as u64 * SCOOP_SIZE;

                    if let Err(e) = file.seek(SeekFrom::Start(seek_addr)) {
                        eprintln!("Seek failed for scoop {}: {}. Skipping scoop.", scoop, e);
                        continue;
                    }

                    let mut local_addr = scoop * buffer_size / NONCE_SIZE * SCOOP_SIZE;
                    for _ in 0..nonces_to_write / TASK_SIZE {
                        if let Err(e) = file.write_all(
                            &bs[local_addr as usize
                                ..(local_addr + TASK_SIZE * SCOOP_SIZE) as usize],
                        ) {
                            eprintln!("Write failed in scoop {}: {}. Skipping chunk.", scoop, e);
                            break;
                        }
                        local_addr += TASK_SIZE * SCOOP_SIZE;
                    }

                    if nonces_to_write % TASK_SIZE > 0 {
                        if let Err(e) = file.write_all(
                            &bs[local_addr as usize
                                ..(local_addr + (nonces_to_write % TASK_SIZE) * SCOOP_SIZE)
                                as usize],
                        ) {
                            eprintln!("Remainder write failed in scoop {}: {}. Skipping.", scoop, e);
                        }
                    }

                    if (scoop + 1) % 128 == 0 {
                        if let Some(pb_ref) = &pb {
                            pb_ref.inc(nonces_to_write * SCOOP_SIZE * 128u64);
                        }
                    }
                }
            }
            nonces_written += nonces_to_write;

            if task.nonces == nonces_written {
                if let Some(pb_ref) = &pb {
                    pb_ref.finish_with_message("Writer done.");
                }
                tx_empty_buffers.send(buffer).unwrap();
                break;
            }

            if !task.benchmark {
                if write_resume_info(&filename, nonces_written).is_err() {
                    println!("Error: couldn't write resume info");
                }
            }
            tx_empty_buffers.send(buffer).unwrap();
        }
    }
}

pub fn read_resume_info(file: &Path) -> Result<u64, Error> {
    let mut file = open_r(&file)?;
    file.seek(SeekFrom::End(-8))?;
      

    let mut progress = [0u8; 4];
    let mut double_monkey = [0u8; 4];

    file.read_exact(&mut progress[0..4])?;
    file.read_exact(&mut double_monkey[0..4])?;

    if double_monkey == [0xAF, 0xFE, 0xAF, 0xFE] {
        Ok(u64::from(as_u32_le(progress)))
    } else {
        Err(Error::new(ErrorKind::Other, "End marker not found"))
    }
}

pub fn write_resume_info(file: &Path, nonces_written: u64) -> Result<(), Error> {
    let mut file = open(&file)?;
    file.seek(SeekFrom::End(-8))?;

    let progress = as_u8_le(nonces_written as u32);
    let double_monkey = [0xAF, 0xFE, 0xAF, 0xFE];

    file.write_all(&progress[0..4])?;
    file.write_all(&double_monkey[0..4])?;
    Ok(())    
}

fn as_u32_le(array: [u8; 4]) -> u32 {
    u32::from(array[0])
        + (u32::from(array[1]) << 8)
        + (u32::from(array[2]) << 16)
        + (u32::from(array[3]) << 24)
}

fn as_u8_le(x: u32) -> [u8; 4] {
    let b1: u8 = (x & 0xff) as u8;
    let b2: u8 = ((x >> 8) & 0xff) as u8;
    let b3: u8 = ((x >> 16) & 0xff) as u8;
    let b4: u8 = ((x >> 24) & 0xff) as u8;
    [b1, b2, b3, b4]
}