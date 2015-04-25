use xml::reader::EventReader;
use xml::reader::events::XmlEvent;

use std::io::{Write, Read};

use {rustyfi_name, get_attribute};

pub struct StructContentParser {
    /// List of fields with (name, type)
    fields: Vec<(String, String)>,
    definition: Vec<u8>,
    send_impl: Option<(Vec<u8>, bool)>,
    socket_send_impl: Option<Vec<u8>>,
}

pub enum StructType {
    Struct,
    Request { opcode: u8 },
}

impl StructContentParser {
    pub fn new(name: &str, ty: StructType) -> StructContentParser {
        let mut definition = Vec::new();
        writeln!(definition, "pub struct {} {{", name).unwrap();

        let send_impl;
        let socket_send_impl;

        match ty {
            StructType::Request { opcode } => {
                let mut send = Vec::new();
                writeln!(send, "impl {} {{", name).unwrap();
                writeln!(send, "    fn send(&self, socket: &mut TcpStream, sequence: u32) \
                                            -> IoResult<()> {{").unwrap();

                writeln!(send, "try!(socket.write_u8({}));", opcode).unwrap();

                send_impl = Some((send, false));
                socket_send_impl = None;
            },

            StructType::Struct => {
                let mut socket_send = Vec::new();
                writeln!(socket_send, "impl SocketSend for {} {{", name).unwrap();
                writeln!(socket_send, "    fn socket_send(&self, socket: &mut TcpStream) \
                                                          -> IoResult<()> {{").unwrap();
                
                send_impl = None;
                socket_send_impl = Some(socket_send);
            },
        }

        StructContentParser {
            fields: Vec::with_capacity(0),
            definition: definition,
            send_impl: send_impl,
            socket_send_impl: socket_send_impl,
        }
    }

    pub fn feed<R>(&mut self, event: XmlEvent, events_list: &mut EventReader<R>) where R: Read {
        match event {
            // `<pad bytes="N" />`
            XmlEvent::EndElement{ref name} if name.local_name == "pad" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "pad" =>
            {
                let bytes = get_attribute(attributes, "bytes").unwrap();
                let bytes: usize = bytes.parse().unwrap();

                if let Some((ref mut send_impl, _)) = self.send_impl {
                    writeln!(send_impl, "\tfor _ in 0 .. {} {{ \
                                               try!(socket.write_u8(0)); \
                                           }}", bytes).unwrap();
                }

                if let Some(ref mut socket_send_impl) = self.socket_send_impl {
                    writeln!(socket_send_impl, "\tfor _ in 0 .. {} {{ \
                                                      try!(socket.write_u8(0)); \
                                                  }}", bytes).unwrap();
                }
            },

            // `<field type="..." name="..." />`
            XmlEvent::EndElement{ref name} if name.local_name == "field" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "field" =>
            {
                let ty = get_attribute(attributes, "type").unwrap();
                let name = rustyfi_name(get_attribute(attributes, "name").unwrap());
                writeln!(self.definition, "\tpub {}: {},", name, ty).unwrap();

                if let Some((ref mut send_impl, _)) = self.send_impl {
                    writeln!(send_impl, "\ttry!(self.{}.socket_send(&mut socket));", name).unwrap();
                }

                if let Some(ref mut socket_send_impl) = self.socket_send_impl {
                    writeln!(socket_send_impl, "\t\ttry!(self.{}.socket_send(&mut socket));", name).unwrap();
                }

                self.fields.push((name, ty));
            },

            _ => ()
            //ev => panic!("Unexpected element in XML definitions: {:?}", ev)
        }

        if let Some((ref mut send_impl, ref mut seqnum_sent)) = self.send_impl {
            if !*seqnum_sent {
                writeln!(send_impl, "try!(sequence.socket_send(socket));").unwrap();
                *seqnum_sent = true;
            }
        }
    }

    pub fn finish<W>(mut self, dest: &mut W) -> Vec<(String, String)> where W: Write {
        dest.write_all(&self.definition).unwrap();
        if self.fields.is_empty() { writeln!(dest, "e: ()").unwrap(); }
        writeln!(dest, "}}").unwrap();

        if let Some((ref mut send_impl, ref mut seqnum_sent)) = self.send_impl {
            if self.fields.is_empty() {
                writeln!(send_impl, "try!(socket.write_u8(0));").unwrap();
            }

            if !*seqnum_sent {
                writeln!(send_impl, "try!(sequence.socket_send(socket));").unwrap();
                *seqnum_sent = true;
            }

            writeln!(send_impl, "Ok(()) }} }}").unwrap();
            dest.write_all(send_impl).unwrap();
        }

        if let Some(ref mut socket_send_impl) = self.socket_send_impl {
            writeln!(socket_send_impl, "\t\tOk(())\n\t}}\n}}").unwrap();
            dest.write_all(socket_send_impl).unwrap();
        }

        self.fields
    }
}
