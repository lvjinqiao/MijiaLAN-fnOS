use clap::{Parser, Subcommand};
use mijialan_core::{Engine, Token, cloud, discovery};
use mijialan_homekit::HomeKit;
use serde_json::Value;
use std::{
    net::Ipv4Addr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Parser)]
#[command(
    name = "mijia-rust",
    version,
    about = "MijiaLAN Rust 局域网控制与 HomeKit 桥"
)]
struct Args {
    #[arg(long, global = true)]
    demo: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Discover,
    List,
    Add {
        host: Ipv4Addr,
        #[arg(long, default_value = "")]
        name: String,
        #[arg(long, default_value_t = 54321)]
        port: u16,
    },
    Refresh {
        did: Option<String>,
    },
    Set {
        did: String,
        property: String,
        value: String,
    },
    Action {
        did: String,
        siid: u32,
        aiid: u32,
        #[arg(default_value = "[]")]
        inputs: String,
    },
    Login {
        #[arg(long, default_value = "cn")]
        region: String,
    },
    Bridge,
    Remove {
        did: String,
    },
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let engine = Arc::new(if args.demo {
        Engine::demo()
    } else {
        Engine::open()?
    });
    match args.command {
        Command::Discover => {
            if args.demo {
                anyhow::bail!("演示模式不访问真实网络");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&discovery::discover(Duration::from_secs(3))?)?
            );
        }
        Command::List => println!("{}", serde_json::to_string_pretty(&engine.states())?),
        Command::Add { host, name, port } => {
            if args.demo {
                anyhow::bail!("演示模式不添加真实设备");
            }
            let secret = rpassword::prompt_password("设备 Token（输入不回显）: ")?;
            let device = engine.add(&name, host, Token::parse(&secret)?, port)?;
            println!("{}", serde_json::to_string_pretty(&device)?);
        }
        Command::Refresh { did } => {
            if let Some(did) = did {
                println!("{}", serde_json::to_string_pretty(&engine.refresh(&did)?)?);
            } else {
                engine.refresh_all();
                println!("{}", serde_json::to_string_pretty(&engine.states())?);
            }
        }
        Command::Set {
            did,
            property,
            value,
        } => {
            engine.write(&did, &property, serde_json::from_str::<Value>(&value)?)?;
            println!("设备已确认指令");
        }
        Command::Action {
            did,
            siid,
            aiid,
            inputs,
        } => {
            engine.action(&did, siid, aiid, serde_json::from_str(&inputs)?)?;
            println!("设备已确认动作");
        }
        Command::Remove { did } => {
            engine.remove(&did)?;
            println!("设备已移除");
        }
        Command::Login { region } => {
            if args.demo {
                anyhow::bail!("演示模式不登录真实账户");
            }
            let cancelled = Arc::new(AtomicBool::new(false));
            let signal = cancelled.clone();
            ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
            let ticket = cloud::start()?;
            let qr = qrcode::QrCode::new(ticket.login_url.as_bytes())?;
            println!(
                "用米家 App 扫描并确认：\n{}",
                qr.render::<qrcode::render::unicode::Dense1x2>()
                    .quiet_zone(true)
                    .build()
            );
            let session = cloud::wait(&ticket, &region, &cancelled)?;
            let report = session.import(&engine, &cancelled)?;
            println!(
                "导入 {} 台设备，跳过 {} 台",
                report.imported,
                report.skipped.len()
            );
        }
        Command::Bridge => {
            let _poller = engine.start_polling();
            let bridge = HomeKit::new(engine);
            bridge.start()?;
            let info = bridge.info();
            println!("HomeKit 桥已启动，端口 {}", info.port);
            if !info.paired {
                println!(
                    "配对码：{}\n{}",
                    info.code,
                    qrcode::QrCode::new(info.uri.as_bytes())?
                        .render::<qrcode::render::unicode::Dense1x2>()
                        .quiet_zone(true)
                        .build()
                );
            }
            let stop = Arc::new(AtomicBool::new(false));
            let signal = stop.clone();
            ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
            }
            bridge.stop();
        }
    }
    Ok(())
}
