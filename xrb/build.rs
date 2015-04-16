extern crate xmlxcb;
extern crate xml;

use std::env;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use xml::reader::EventReader;
use xml::reader::events::XmlEvent;

fn main() {
    let dest = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&dest);

    parse(&mut File::create(&dest.join("definitions.rs")).unwrap(), Cursor::new(xmlxcb::XPROTO));
}

/*

pub struct XConnection {
    socket: TcpStream,
    
    // sequence number attributed to the next request
    sequence: u32,

    // list of received events that have to be retreived by the user
    pending_events: Vec<Event>,

    // list of answers that have to be retreived by the user
    pending_answers: Vec<(u16, Reply)>,

    // sequence numbers of requests waiting for an answer
    waiting_for_answer: Vec<u16>,
}

pub struct Events<'a> {
    connection: &'a mut XConnection,
}

impl XConnection {
    /// Connects to an X server.
    ///
    /// Blocks until the server returns a success or an error.
    pub fn connect<A>(address: A) -> XConnection where A: ToSocketAddrs {
        let connection = TcpStream::connect(address);
    }

    pub fn events(&mut self) -> Events {
        Events {
            connection: self,
        }
    }
}

*/

fn parse<R, W>(definitions: &mut W, input: R) where W: Write, R: Read {
    let mut events = EventReader::new(input);

    match recv(&mut events) {
        XmlEvent::StartElement{ref name, ..} if name.local_name == "xcb" => (),
        msg => panic!("Expected `<xcb>`, found: {:?}", msg),
    };

    loop {
        match recv(&mut events) {
            /*XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "struct" =>
            {
                let struct_name = get_attribute(attributes, "name").unwrap();
                parse_struct(definitions, &mut events, &struct_name);
            },*/

            // `<request>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "request" =>
            {
                let name = get_attribute(attributes, "name").unwrap();
                let opcode = get_attribute(attributes, "opcode").unwrap().parse().unwrap();
                parse_request(definitions, &mut events, &name, opcode);
            },

            // `<event>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "event" =>
            {
                let name = get_attribute(attributes, "name").unwrap();
                let number = get_attribute(attributes, "number").unwrap().parse().unwrap();
                parse_request(definitions, &mut events, &name, number);
            },

            // we ignore `<import />` as it's C-specific
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "import" =>
            {
            },

            // finished parsing the file
            XmlEvent::EndElement{ref name} if name.local_name == "xcb" => break,

            // error handling
            msg => ()// FIXME: panic!("Unexpected {:?}", msg),
        }
    }
}

fn recv<R>(events: &mut EventReader<R>) -> XmlEvent where R: Read {
    for event in events.events() {
        match event {
            XmlEvent::StartDocument{..} => (),
            XmlEvent::Comment(_) => (),
            XmlEvent::Whitespace(_) => (),
            XmlEvent::EndDocument => panic!("The end of the document has been reached"),
            XmlEvent::Error(err) => panic!("XML error: {:?}", err),
            event => return event,
        }
    }

    unreachable!()
}

fn get_attribute(a: &[xml::attribute::OwnedAttribute], name: &str) -> Option<String> {
    a.iter().find(|a| a.name.local_name == name).map(|e| e.value.clone())
}

fn parse_struct<R, W>(definitions: &mut W, events: &mut EventReader<R>, struct_name: &str)
                      where W: Write, R: Read
{
    let mut pad_num = 0;

    writeln!(definitions, "pub struct {} {{", struct_name).unwrap();

    loop {
        match recv(events) {
            XmlEvent::EndElement{ref name} if name.local_name == "struct" => break,

            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "field" =>
            {
                let ty = get_attribute(attributes, "type").unwrap();
                let name = get_attribute(attributes, "name").unwrap();
                writeln!(definitions, "\t{}: {},", name, ty).unwrap();
            },

            XmlEvent::EndElement{ref name} if name.local_name == "field" => (),

            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "pad" =>
            {
                let bytes = get_attribute(attributes, "bytes").unwrap();
                writeln!(definitions, "\tpad{}: [u8; {}],", pad_num, bytes).unwrap();
                pad_num += 1;
            },

            XmlEvent::EndElement{ref name} if name.local_name == "pad" => (),

            msg => panic!("Unexpected {:?}", msg),
        }
    }

    writeln!(definitions, "}}").unwrap();
}

fn parse_request<R, W>(output: &mut W, events: &mut EventReader<R>, name: &str, opcode: u8)
                       where W: Write, R: Read
{
    let mut instructions = Vec::new();
    writeln!(instructions, "\ttry!(self.socket.write_u8({}));", opcode).unwrap();

    let mut definition = Vec::new();
    write!(definition, "pub fn {}_request(&mut self", name).unwrap();

    let mut docs = Vec::new();

    loop {
        match recv(events) {
            XmlEvent::EndElement{ref name} if name.local_name == "request" => break,

            // `<pad bytes="N" />`
            XmlEvent::EndElement{ref name} if name.local_name == "pad" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "pad" =>
            {
                let bytes = get_attribute(attributes, "bytes").unwrap();
                writeln!(instructions, "\tfor _ in 0 .. {} {{ \
                                            try!(self.socket.write_u8(0)); \
                                        }}", bytes).unwrap();
            },

            // `<field type="..." name="..." />`
            XmlEvent::EndElement{ref name} if name.local_name == "field" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "field" =>
            {
                let ty = get_attribute(attributes, "type").unwrap();
                let name = get_attribute(attributes, "name").unwrap();
                write!(definition, ", {}: {}", name, ty);
                writeln!(instructions, "\ttry!(self.socket.write({}));", name).unwrap();
            },

            // `<doc>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "doc" =>
            {
                parse_doc(&mut docs, events);
            },

            msg => ()// FIXME: panic!("Unexpected {:?}", msg),
        }
    }

    output.write_all(&docs).unwrap();
    writeln!(output, "").unwrap();
    output.write_all(&definition).unwrap();
    writeln!(output, ") {{").unwrap();
    output.write_all(&instructions).unwrap();
    writeln!(output, "}}").unwrap();
    writeln!(output, "").unwrap();
}

fn parse_doc<R, W>(output: &mut W, events: &mut EventReader<R>)
                   where W: Write, R: Read
{
    loop {
        match recv(events) {
            XmlEvent::Characters(chr) => write!(output, "{}", chr).unwrap(),

            XmlEvent::EndElement{ref name} if name.local_name == "doc" => break,

            // `<brief>`
            XmlEvent::EndElement{ref name} if name.local_name == "brief" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "brief" =>
            {
                write!(output, "/// ").unwrap();
            },

            msg => ()// FIXME: panic!("Unexpected {:?}", msg),
        }
    }
}

fn parse_event<R, W>(output: &mut W, events: &mut EventReader<R>, name: &str, number: u8)
                     where W: Write, R: Read
{
    loop {
        match recv(events) {
            XmlEvent::EndElement{ref name} if name.local_name == "event" => break,

            msg => ()       // FIXME: err
        }
    }
}
