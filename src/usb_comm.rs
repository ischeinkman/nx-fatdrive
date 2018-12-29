use std::mem;
use crate::aligned_slice::AlignedBuffer;
use std::alloc::Layout;

use libnx_rs::usb::{EndpointDirection, TransferType, UsbEndpointDescriptor};
use libnx_rs::usb::{UsbConfigDescriptor, UsbDeviceDescriptor, UsbInterfaceDescriptor};
use libnx_rs::usbhs::{
    ClientEndpointSession, ClientInterfaceSession, Interface, InterfaceFilter, UsbHsContext,
};
use libnx_rs::LibnxError;

pub struct ReadEndpoint(UsbEndpointDescriptor);

pub struct WriteEndpoint(UsbEndpointDescriptor);

impl UsbClient {
    pub fn new(
        mut device_handle: ClientInterfaceSession,
        read_endpoint: ReadEndpoint,
        write_endpoint: WriteEndpoint,
    ) -> Result<UsbClient, LibnxError> {
        let read_handle = device_handle.open_endpoint(&read_endpoint.0)?;
        let write_handle = device_handle.open_endpoint(&write_endpoint.0)?;
        Ok(UsbClient {
            read_endpoint : read_handle,
            write_endpoint : write_handle,
            device_handle,
        })
    }

    pub fn pull_bytes(&mut self, buffer: &mut AlignedBuffer) -> Result<usize, String> {
        println!("UsbClient::pull START");
        eprintln!("UsbClient::pull START");
        if buffer.size() == 0 {
            println!("Got read size of 0!");
            eprintln!("Got read size of 0!");
            return Err(format!("Got read size of 0!"));
        }
        if buffer.alignment() != 0x1000 {
            println!("Alignment incorrect! Wanted 0x1000 but have {}.\n", buffer.alignment());
            eprintln!("Alignment incorrect! Wanted 0x1000 but have {}.\n", buffer.alignment());
            return Err(format!("Alignment incorrect! Wanted 0x1000 but have {}.\n", buffer.alignment()));
        }
        let mut session = &mut self.read_endpoint;
        let rval = session
            .read(buffer.as_slice_mut())
            .map_err(|e| format!("Read Error: {:?}", e))?;
        println!("UsbClient::pull END: {}", rval);
        eprintln!("UsbClient::pull END: {}", rval);
        Ok(rval)
    }

    pub fn push_bytes(&mut self, buffer: &AlignedBuffer) -> Result<usize, String> {
        println!("UsbClient::push START");
        eprintln!("UsbClient::push START");
        if buffer.size() == 0 {
            println!("Got write size of 0!");
            eprintln!("Got write size of 0!");
            return Err(format!("Got write size of 0!"));
        }
        if buffer.alignment() != 0x1000 {
            println!("Alignment incorrect! Wanted 0x1000 but have {}.\n", buffer.alignment());
            eprintln!("Alignment incorrect! Wanted 0x1000 but have {}.\n", buffer.alignment());
            return Err(format!("Alignment incorrect! Wanted 0x1000 but have {}.\n", buffer.alignment()));
        }
        let mut session = &mut self.write_endpoint;
        let rval = session
            .write(buffer.as_slice())
            .map_err(|e| format!("Write Error: {:?}", e))?;
        println!("UsbClient::push END: {}", rval);
        eprintln!("UsbClient::push END: {}", rval);
        Ok(rval)
    }
    pub fn from_interface(
        context: &mut UsbHsContext,
        interface: &Interface,
    ) -> Result<UsbClient, LibnxError> {
        let (read_ep, write_ep) = UsbClient::retrieve_iface_endpoints(interface)?;
        let session = context.acquire_interface(interface)?;
        UsbClient::new(
            session,
            read_ep,
            write_ep,
        )
    }

    pub fn retrieve_iface_endpoints(
        interface: &Interface,
    ) -> Result<(ReadEndpoint, WriteEndpoint), LibnxError> {
        let device_desc = interface.device_desc();
        let info = interface.info();
        let iface_desc = info.interface_desc();

        let has_valid_class = device_desc.class() == 8 || iface_desc.class() == 8;
        let has_valid_subclass = device_desc.subclass() == 6 || iface_desc.subclass() == 6;
        let has_valid_protocol = device_desc.protocol() == 80 || iface_desc.protocol() == 80;

        if !(has_valid_class && has_valid_subclass && has_valid_protocol) {
            return Err(LibnxError::from_msg(
                "Error: interface is not SCSI Bulk-Only Mass Storage device.".to_owned(),
            ));
        }

        let mut all_write_eps = info.input_endpoint_descs();
        let write_ep = all_write_eps.next().ok_or(LibnxError::from_msg(
            "Error: no output endpoint found!".to_owned(),
        ))?;

        let mut all_read_eps = info.output_endpoint_descs();
        let read_ep = all_read_eps.next().ok_or(LibnxError::from_msg(
            "Error: no input endpoint found!".to_owned(),
        ))?;
        if (read_ep.address() & 0x80) == 0 || read_ep.direction() != EndpointDirection::IN {
            return Err(LibnxError::from_msg(format!("read_ep problem: {:?} vs {:?}, {:?} vs {:?}.", 0x80, read_ep.address(), read_ep.direction(), EndpointDirection::IN)));
        }
        if (write_ep.address() & 0x80) != 0 || write_ep.direction() != EndpointDirection::OUT {
            return Err(LibnxError::from_msg(format!("write_ep problem: {:?} vs {:?}, {:?} vs {:?}.", 0x80, write_ep.address(), write_ep.direction(), EndpointDirection::OUT)));
        }
        Ok((
            ReadEndpoint(read_ep),
            WriteEndpoint(write_ep),
        ))
    }
}

pub struct UsbClient {
    device_handle: ClientInterfaceSession,
    read_endpoint: ClientEndpointSession,
    write_endpoint: ClientEndpointSession,
}


impl Drop for UsbClient {
    fn drop(&mut self) {}
}

impl scsi::CommunicationChannel for UsbClient {
    fn in_transfer<B: scsi::Buffer>(&mut self, buffer: &mut B) -> Result<usize, scsi::ScsiError> {
        let to_get = buffer.capacity() - buffer.size();
        let shim_layout = Layout::from_size_align(to_get, 0x1000).map_err(|e| scsi::ScsiError::from_cause(scsi::ErrorCause::FlagError{flags : 0xEAFF}))?;
        let mut shim = AlignedBuffer::from_layout(shim_layout).map_err(|e| scsi::ScsiError::from_cause(scsi::ErrorCause::FlagError{flags : 0xEAFE}))?;
        let rval = self
            .pull_bytes(&mut shim)
            .map_err(|e| {
                println!("Got error in read: {:?}", e);
                eprintln!("Got error in read: {:?}", e);
                scsi::ScsiError::from_cause(scsi::ErrorCause::UsbTransferError {
                    direction: scsi::UsbTransferDirection::In,
                })
            })?;

        let mut idx = 0;
        while buffer.capacity() > buffer.size() {
            let byte = shim.as_slice()[idx];
            buffer.push_byte(byte).map_err(|e| format!("LibnxErr: {:?}", e));
            idx += 1;
        }
        Ok(rval)
    }

    fn out_transfer<B: scsi::Buffer>(&mut self, bytes: &mut B) -> Result<usize, scsi::ScsiError> {
        let shim_layout = Layout::from_size_align(bytes.size(), 0x1000).map_err(|e| scsi::ScsiError::from_cause(scsi::ErrorCause::FlagError{flags : 0xEAFF}))?;
        let mut shim = AlignedBuffer::from_layout(shim_layout).map_err(|e| scsi::ScsiError::from_cause(scsi::ErrorCause::FlagError{flags : 0xEAFE}))?;
        let mut idx = 0; 
        while bytes.size() > 0 {
            let bt = bytes.pull_byte()?;
            shim.as_slice_mut()[idx] = bt;
            idx += 1;
        }
        let rval = self
            .push_bytes(&shim)
            .map_err(|e| {
                println!("Got error in read: {:?}", e);
                eprintln!("Got error in write: {:?}", e);
                scsi::ScsiError::from_cause(scsi::ErrorCause::UsbTransferError {
                    direction: scsi::UsbTransferDirection::Out,
                })
            })?;
        Ok(rval)
    }
}
