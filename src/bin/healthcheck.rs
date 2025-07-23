use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
};

fn main() {
    let port = env::var("PORT")
        .unwrap_or("8080".to_string())
        .parse::<u16>()
        .expect("valid port (u16)");

    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("tcp connect");

    let data = "GET /health HTTP/1.1\r\n\r\n";
    stream.write_all(data.as_bytes()).expect("write data");

    let mut buffer = [0; 12];
    stream.read_exact(&mut buffer).expect("read data");

    assert!(buffer == "HTTP/1.1 200".as_bytes(), "response is not ok");
}
