//! Credential-free connectivity diagnostic; does not request or print login tickets.
fn main() {
    use std::net::ToSocketAddrs;
    let addresses: Vec<_> = ("account.xiaomi.com", 443)
        .to_socket_addrs()
        .unwrap()
        .collect();
    println!("DNS {:?}", addresses);
    for address in &addresses {
        let now = std::time::Instant::now();
        println!(
            "TCP {address} {:?} {:?}",
            std::net::TcpStream::connect_timeout(address, std::time::Duration::from_secs(2))
                .map(|_| ()),
            now.elapsed()
        );
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
        .timeout(std::time::Duration::from_secs(8))
        .connect_timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap();
    for url in ["https://account.xiaomi.com/", "https://miot-spec.org/"] {
        match client.get(url).send() {
            Ok(response) => println!("{url} HTTP {}", response.status()),
            Err(error) => println!("{url} {:?}", error.without_url()),
        }
    }
}
