use anyhow::{bail, Context, Result};
use arkline_protocol::{Destination, Request, Response, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Credentials {
    server: String,
    pilot: String,
    resume_token: String,
}

fn exchange(stream: &mut BufReader<TcpStream>, request: &Request) -> Result<Response> {
    let mut wire = serde_json::to_vec(request)?;
    wire.push(b'\n');
    stream.get_mut().write_all(&wire)?;
    let mut line = String::new();
    if stream.read_line(&mut line)? == 0 {
        bail!("server disconnected");
    }
    let response: Response = serde_json::from_str(&line).context("invalid server response")?;
    if !response.ok {
        bail!("{}", response.message);
    }
    Ok(response)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Arkline: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut server = "127.0.0.1:7777".to_string();
    let mut pilot: Option<String> = None;
    let mut credentials_path: Option<PathBuf> = None;
    let mut command = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => server = args.next().context("--server needs HOST:PORT")?,
            "--pilot" => pilot = Some(args.next().context("--pilot needs a name")?),
            "--credentials" => {
                credentials_path = Some(args.next().context("--credentials needs a path")?.into())
            }
            "--help" | "-h" => {
                println!("Arkline command client\n\narkline [--server HOST:PORT] [--pilot NAME] [--credentials PATH] COMMAND\n\nCommands:\n  status                 Read the live universe\n  connect                Create a pilot or resume with saved credentials\n  undock                 Leave Junction Station\n  warp belt|station      Begin a six-second warp (2s warmup)\n  dock                   Dock at the station\n  mine UNITS             Mine up to 100 ore units\n  sell UNITS PRICE       Escrow ore in a sell order\n  buy ORDER_ID UNITS     Buy ore from another pilot\n  cancel ORDER_ID        Cancel your order and reclaim ore\n  export PATH            Save an economic audit export\n\nCommands return JSON snapshots. Credentials are saved locally with restricted\npermissions. Keep that file to resume the same pilot. This prototype uses\nunencrypted TCP: keep it on loopback or an authenticated private tunnel.");
                return Ok(());
            }
            x if x.starts_with('-') => bail!("unknown option: {x}"),
            _ => command.push(arg),
        }
    }
    let number = |index: usize| -> Result<u64> {
        command
            .get(index)
            .context("missing numeric argument")?
            .parse()
            .context("expected nonnegative integer")
    };
    let verb = command.first().map(String::as_str).unwrap_or("status");
    let request = match verb {
        "status" | "connect" => Request::Snapshot,
        "undock" => Request::Undock,
        "dock" => Request::Dock,
        "warp" => Request::Warp {
            destination: match command.get(1).map(String::as_str) {
                Some("belt") => Destination::Belt,
                Some("station") => Destination::Station,
                _ => bail!("warp destination must be belt or station"),
            },
        },
        "mine" => Request::Mine { units: number(1)? },
        "sell" => Request::PlaceSell {
            units: number(1)?,
            unit_price: number(2)?,
        },
        "buy" => Request::Buy {
            order_id: number(1)?,
            units: number(2)?,
        },
        "cancel" => Request::CancelSell {
            order_id: number(1)?,
        },
        "export" => {
            if command.len() != 2 {
                bail!("export needs a destination path");
            }
            Request::ExportLedger
        }
        _ => bail!("unknown command '{verb}'; see --help"),
    };
    let stream = TcpStream::connect(&server)
        .with_context(|| format!("cannot connect to {server}; start arkline-server first"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut stream = BufReader::new(stream);
    if let Some(pilot) = pilot {
        if pilot.is_empty()
            || !pilot
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            bail!("pilot name must use letters, numbers, '_' or '-'");
        }
        let safe_server: String = server
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let path = credentials_path.unwrap_or_else(|| {
            PathBuf::from(format!("data/{safe_server}-{pilot}.credentials.json"))
        });
        let resume_token = if path.exists() {
            let saved: Credentials = serde_json::from_slice(&fs::read(&path)?)?;
            if saved.server != server || saved.pilot != pilot {
                bail!("credentials belong to another server or pilot");
            }
            Some(saved.resume_token)
        } else {
            None
        };
        let response = exchange(
            &mut stream,
            &Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                pilot: pilot.clone(),
                resume_token,
            },
        )?;
        if let Some(token) = response.resume_token {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent)?;
            }
            let credentials = Credentials {
                server,
                pilot,
                resume_token: token,
            };
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(&serde_json::to_vec_pretty(&credentials)?)?;
            eprintln!("Pilot credentials saved to {}", path.display());
        }
    } else if !matches!(verb, "status" | "export") {
        bail!("this command needs --pilot NAME");
    }
    let mut response = exchange(&mut stream, &request)?;
    response.resume_token = None;
    if verb == "export" {
        let ledger = response.ledger.context("server did not return a ledger")?;
        fs::write(&command[1], serde_json::to_vec_pretty(&ledger)?)?;
        println!(
            "Saved {} economic events to {}",
            ledger.entries.len(),
            command[1]
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&response)?);
    }
    Ok(())
}
