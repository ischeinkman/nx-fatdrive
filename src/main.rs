#![allow(dead_code)]

extern crate nx_fatdrive;
use nx_fatdrive::{usb_comm, buf_scsi, vecwrapper};

extern crate libnx_rs;

extern crate libc;

extern crate scsi;
use scsi::{ScsiError, ErrorCause};

extern crate mbr_nostd;
use mbr_nostd::PartitionTable;
use mbr_nostd::PartitionTableEntry;

use std::result::Result;
use std::path::Path;
use std::fs::File;
use std::fs::OpenOptions;
use std::error::Error;
use std::io::Write;
use std::io;
use std::os::unix::io::AsRawFd;
use std::panic;
use std::ptr;
macro_rules! multprint {
    ($stout:expr, $sterr:expr, $fmt:expr, $($arg:tt)*) => {{
        println!($fmt, $($arg)*);
        $stout.update();

        eprintln!($fmt, $($arg)*);
        if let Err(fl_e) = $sterr.flush() {
            println!("Failed flushing error file: {:?}", fl_e);
            $stout.update();
        }
    }};
    
    ($stout:expr, $sterr:expr, $fmt:expr) => {{
        println!($fmt);
        $stout.update();

        eprintln!($fmt);
        if let Err(fl_e) = $sterr.flush() {
            println!("Failed flushing error file: {:?}", fl_e);
            $stout.update();
        }

    }};
}


pub fn main() {
    let res = panic::catch_unwind(|| runner());
    if let Err(e) = res {
        let mut panic_out = match OpenOptions::new().write(true).create(true).open("panic_out.txt") {
            Ok(f) => f, 
            Err(_) => {
                return;
            }
        };
        panic_out.write_fmt(format_args!("Got panic from cause:\n{:?}", e));
        panic_out.flush();
    }
}


pub fn redirect_stdout (filename : &str) -> Result<File, io::Error> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut outfile = OpenOptions::new()
        .write(true)
        .create(true)
        .custom_flags(0x0080)
        .open(filename)?;
    outfile.write_fmt(format_args!("Redirecting standard output to {}.", filename))?;
    let raw_fd = outfile.as_raw_fd();
    let new_fd = unsafe {
        libc::fflush(0 as *mut libc::FILE);
        libc::dup2(raw_fd, libc::STDOUT_FILENO)
    };
    if new_fd != libc::STDOUT_FILENO {
        Err(io::Error::new(io::ErrorKind::Other, format!("Could not call dup2. Ended up redirecting fd {} to {} instead of {}.", raw_fd, new_fd, libc::STDOUT_FILENO)))
    }
    else { 
        Ok(outfile) 
    }
}

pub fn redirect_stderr (filename : &str) -> Result<File, io::Error> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut outfile = OpenOptions::new()
        .write(true)
        .create(true)
        .custom_flags(0x0080)
        .open(filename)?;
    outfile.write_fmt(format_args!("Redirecting standard error to {}.\n", filename))?;
    let raw_fd = outfile.as_raw_fd();
    let new_fd = unsafe {
        libc::fflush(0 as *mut libc::FILE);
        libc::dup2(raw_fd, libc::STDERR_FILENO)
    };
    if new_fd != libc::STDERR_FILENO {
        Err(io::Error::new(io::ErrorKind::Other, format!("Could not call dup2. Ended up redirecting fd {} to {} instead of {}.", raw_fd, new_fd, libc::STDERR_FILENO)))
    }
    else { 
        Ok(outfile) 
    }
}

use libnx_rs::console::ConsoleHandle;
use libnx_rs::hid::{HidContext, Controller, HidControllerID};
use libnx_rs::libnx::HidControllerKeys;
use libnx_rs::usbhs::{UsbHsContext, Interface, InterfaceFilter, InterfaceInfo, InterfaceAvailableEvent};
use usb_comm::{UsbClient};
use vecwrapper::VecNewtype;
use buf_scsi::OffsetScsiDevice;

use std::time::{Instant, Duration};

pub fn runner() {
    let mut console = ConsoleHandle::init_default();
    console.clear();

    println!("Setting up sterr file hooks.");
    console.update();
    let mut error_file = match redirect_stderr("nx_fatdrive_sterr.txt") {
        Ok(fl) => fl,
        Err(e) => {
            println!("Error setting stderr output: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };
    console.update();
    
    
    multprint!(console,error_file,"Setting up HID context.");
    let mut hid_ctx = HidContext::new();
    let controller = hid_ctx.get_controller(HidControllerID::CONTROLLER_P1_AUTO);

    multprint!(console,error_file,"Setting up usb:hs context");
    let mut usbhs_ctx = match UsbHsContext::initialize() {
        Ok(ctx) => ctx, 
        Err(e) => {
            multprint!(console, error_file, "Failed setting up usb:hs context: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    let filter : InterfaceFilter = InterfaceFilter::new()
        .with_interface_class(8)
        .with_interface_subclass(6)
        .with_interface_protocol(80);
    
    multprint!(console, error_file, "Waiting for usb event.");
    let evt = match InterfaceAvailableEvent::create(true, 0, filter) {
        Ok(ev) => ev, 
        Err(e) => {
            multprint!(console, error_file, "Failed building iface available event: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;

        }
    };
    if let Err(e) = evt.wait(u64::max_value()) {
        multprint!(console, error_file, "Failed waiting for event: {:?}", e);
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_secs(5) {
            console.update();
        }
        return;
    }
    multprint!(console,error_file,"Looking for usb devices.");

    let mut interfaces = match usbhs_ctx.query_available_interfaces(filter, 3) {
        Ok(ifaces) => ifaces, 
        Err(e) => {
            multprint!(console, error_file, "Failed querying available interfaces: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    multprint!(console, error_file, "Got interfaces: {:?}", interfaces);

    let mut iface = match interfaces.pop() {
        Some(iface) => iface, 
        None => {
            multprint!(console, error_file, "Failed finding any matching interfaces.");
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    multprint!(console, error_file, "\nSuccess! Using iface: {:?}", iface);

    console.update();

    let (read_ep, write_ep) = match UsbClient::retrieve_iface_endpoints(&iface) {
        Ok(p) => p,
        Err(e) => {
            multprint!(console, error_file, "Failed getting eps: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    let mut session = match usbhs_ctx.acquire_interface(&iface) {
        Ok(s) => s,
        Err(e) => {
            multprint!(console, error_file, "Failed acquiring iface: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };
    let client = match UsbClient::new(session, read_ep, write_ep) {
        Ok(c) => c, 
        Err(e) => {
            multprint!(console, error_file, "Got error on usbclient::new of {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;

        }
    };
    
    multprint!(console,error_file,"Making SCSI wrapper object.");
    console.update();

    let mut scsi_wrapper = match scsi::scsi::ScsiBlockDevice::new(client, VecNewtype::new(), VecNewtype::new(), VecNewtype::new()) {
        Ok(c) => c,
        Err(e) => {
            multprint!(console, error_file, "Failed creating SCSI wrapper object: {:?}", e);

            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    multprint!(console,error_file,"SCSI device found with block size {}.", scsi_wrapper.block_size());
    multprint!(console,error_file,"Trying to get MBR.");
    console.update();

    let mut mbr_buff = VecNewtype::with_fake_capacity(512.max(scsi_wrapper.block_size() as usize));
    let mut mbr_read_count = 0;
    while mbr_buff.inner.len() < 512 {
        multprint!(console, error_file, "MBR Parse pre-status {}: {}/512.", mbr_read_count, mbr_buff.inner.len());
        let _bt = match scsi_wrapper.read(mbr_buff.inner.len() as u32, &mut mbr_buff) {
            Ok(bt) => {
                multprint!(console, error_file, "Got {} bytes on read {}.", mbr_buff.inner.len(), bt);
                multprint!(console, error_file, "Ended with bytes: {:X?}", mbr_buff.inner);
                bt
            }, 
            Err(e) => {
                multprint!(console, error_file, "Failed reading MBR on read number {} after already getting {} bytes: {:?}.", mbr_read_count, mbr_buff.inner.len(), e);
                multprint!(console, error_file, "Ended with bytes: {:X?}", mbr_buff.inner);

                let delay_start = Instant::now();
                while delay_start.elapsed() < Duration::from_secs(5) {
                    console.update();
                }
                return;
            }
        };
        mbr_read_count += 1;
    }

    multprint!(console,error_file,"Parsing MBR.");
    console.update();

    let mbr_entry = match mbr_nostd::MasterBootRecord::from_bytes(&mut mbr_buff.inner) {
        Ok(mbr) => mbr, 
        Err(e) => {
            multprint!(console, error_file, "Failed parsing mbr: {:?}", e);

            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };


    multprint!(console,error_file,"Partitions:");
    for ent in mbr_entry.partition_table_entries() {
        multprint!(console,error_file,"    {:?}", ent);
    }


    let first_ent : &PartitionTableEntry = &mbr_entry.partition_table_entries()[0];
    let raw_offset : usize = (first_ent.logical_block_address * scsi_wrapper.block_size()) as usize; 
    multprint!(console, error_file, "Creating FATFS wrapper starting at offset block {}, raw {}.", first_ent.logical_block_address, raw_offset);

    let mut partition = OffsetScsiDevice::new(scsi_wrapper, raw_offset);
    let mut fs : fatfs::FileSystem<OffsetScsiDevice> = match fatfs::FileSystem::new(partition, fatfs::FsOptions::new()) {
        Ok(fs) => fs, 
        Err(e) => {
            multprint!(console, error_file, "Error mounting FAT32 file system: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    multprint!(console, error_file, "Scanning filesystem.");
    let mut root_dir = fs.root_dir();
    let all_dirs = root_dir.iter().filter_map(|ent_res| {
        match ent_res {
            Ok(ent) => {
                multprint!(console, error_file, "FAT: Found itm. Short name: {}, long name: {}, attr: {:?}", ent.short_file_name(), ent.file_name(), ent.attributes());
                Some(ent)
            },
            Err(e) => {
                multprint!(console, error_file, "Error reading dirent: {:?}", e);
                None
            }
        }

    }).collect::<Vec<_>>();

    multprint!(console, error_file, "Getting handle to test_folder directory.");
    let subdir_opt = all_dirs.iter().find_map(|fl| {
        if fl.is_dir() && fl.file_name() == "test_folder".to_owned() {
            multprint!(console, error_file,"FAT: Using existing subdir: Short name: {}, long name: {}, attr: {:?}", fl.short_file_name(), fl.file_name(), fl.attributes());
            Some(fl.to_dir())
        }
        else {
            None
        }
    });

    let mut subdir_res = subdir_opt.ok_or("Could not find existing subdir.").or_else(|_| {
        root_dir.create_dir("test_folder")
    });
    let mut subdir = match subdir_res {
        Ok(s) => s, 
        Err(e) => {
            multprint!(console, error_file, "Error getting handle to test_folder: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };


    let now = Instant::now();
    let fl_name = format!("{:?}.txt", now).replace(" ", "s").replace(":", "o").replace("{", "q").replace("}", "p");
    multprint!(console, error_file, "Creating test file {} in the folder.", fl_name);
    let mut fl = match subdir.create_file(&fl_name) {
        Ok(f) => f, 
        Err(e) => {
            multprint!(console, error_file, "Error creating test file: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    multprint!(console, error_file,"Now writing to file.");
    if let Err(e) = fl.write_fmt(format_args!("Hello world at time {:?}", now)) {
        multprint!(console, error_file, "Error writing to test file: {:?}", e);
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_secs(5) {
            console.update();
        }
        return;
    }

    let next_dir_name = format!("{:?}_next_dir", now).replace(" ", "s").replace(":", "o").replace("{", "q").replace("}", "p");
    multprint!(console, error_file, "Now trying directory {}.", next_dir_name);
    let mut next_dir = match root_dir.create_dir(&next_dir_name) {
        Ok(s) => s, 
        Err(e) => {
            multprint!(console, error_file, "Error getting handle to next_dir: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };
    let mut outfile = match next_dir.create_file("for_seuth.txt") {
        Ok(f) => f, 
        Err(e) => {
            multprint!(console, error_file, "Error creating for_seuth.txt: {:?}", e);
            let delay_start = Instant::now();
            while delay_start.elapsed() < Duration::from_secs(5) {
                console.update();
            }
            return;
        }
    };

    if let Err(e) = outfile.write("To be or not to be and all that jazz!.".to_owned().into_bytes().as_slice()) {
        multprint!(console, error_file, "Error writing to for_seuth.txt: {:?}", e);
        let delay_start = Instant::now();
        while delay_start.elapsed() < Duration::from_secs(5) {
            console.update();
        }
        return;

    }

    multprint!(console, error_file, "Done.");

    loop {
        hid_ctx.scan_input();
        if controller.keys_down_raw() & HidControllerKeys::KEY_PLUS.0 as u64 != 0 {
            break;
        }
    }
}

fn fmt_o_csw(csw: &Option<scsi::scsi::commands::CommandStatusWrapper>) -> String {
    match csw {
        None => "None".to_owned(),
        Some(csw) => {
            format!("Some({{ tag: {}, data_residue: {}, status: {} }})", csw.tag, csw.data_residue, csw.status)
        }
    }
}