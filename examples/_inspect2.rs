use quick_xml::Reader;
use quick_xml::events::Event;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut r = Reader::from_reader(bytes.as_slice());
    r.config_mut().expand_empty_elements = true;
    r.config_mut().check_end_names = true;
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(ev) => println!("{ev:?}"),
            Err(e) => {
                println!("ERR: {e}");
                break;
            }
        }
        buf.clear();
    }
}
