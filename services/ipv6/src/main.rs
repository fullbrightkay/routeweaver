use ipnet::Ipv6Net;
use std::{net::Ipv6Addr, time::Duration};
use tokio::time::sleep;
use tun_rs::{AbstractDevice, Configuration};

const NETWORK_PREFIX: Ipv6Net = const {
    match Ipv6Net::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), 48) {
        Ok(net) => net,
        Err(_) => unreachable!(),
    }
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Configuration::default();
    let dev = tun_rs::create_as_async(&config).unwrap();
    dev.add_address_v6("fd12:3434:3434::".parse().unwrap(), 48)
        .unwrap();

    sleep(Duration::from_secs(1000)).await;
}
