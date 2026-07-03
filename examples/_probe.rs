use std::io::Cursor;

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read");
        let dom = fastxml::Parser::from(bytes.as_slice()).parse().map(|_| ());
        let stream =
            fastxml::Parser::from_reader(Cursor::new(bytes.clone())).for_each_event(|_| Ok(()));
        println!(
            "{path}\n  DOM:    {}\n  STREAM: {}",
            match &dom {
                Ok(()) => "ACCEPT".to_string(),
                Err(e) => format!("REJECT: {e}"),
            },
            match &stream {
                Ok(()) => "ACCEPT".to_string(),
                Err(e) => format!("REJECT: {e}"),
            },
        );
    }
}
