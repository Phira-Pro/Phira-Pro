use std::{thread::sleep, time::Duration};

fn main() {
    let assets = std::path::PathBuf::from(std::env::args().nth(1).expect("assets"));
    let data = std::path::PathBuf::from(std::env::args().nth(2).expect("data"));
    let host = rpen_host::EditorHost::start(assets, data).expect("start host");
    println!("URL {}", host.url);
    loop {
        sleep(Duration::from_secs(3600));
    }
}
