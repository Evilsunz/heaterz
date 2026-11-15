use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::http::server::EspHttpServer;
use esp_idf_svc::http::server::{EspHttpConnection, Request};
use esp_idf_svc::io::EspIOError;
use esp_idf_svc::mdns::{EspMdns, Interface, Protocol, QueryResult};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use esp_idf_svc::sys::*;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration, EspWifi};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

static FILE_PATH: &str = "/littlefs/log.csv";
static FILE_LOCK: Mutex<()> = Mutex::new(());

fn main() -> anyhow::Result<()> {
    link_patches();

    unsafe {
        let mut conf = esp_vfs_littlefs_conf_t {
            base_path: c"/littlefs".as_ptr(),
            partition_label: c"storage".as_ptr(),
            partition: std::ptr::null(),
            ..Default::default()
        };

        conf.set_format_if_mount_failed(1);
        conf.set_read_only(0);
        conf.set_dont_mount(0);

        let rc = esp_vfs_littlefs_register(&conf);

        println!("mount rc = {}", rc);

        if rc != ESP_OK {
            panic!("LittleFS mount failed: {}", rc);
        }
    }

    println!("Storage mounted!");

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = EspWifi::new(peripherals.modem, sysloop, Some(nvs))?;
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: heapless::String::try_from("CMS Roanoke").unwrap(),
        password: heapless::String::try_from("dead__space").unwrap(),
        ..Default::default()
    }))?;

    wifi.start()?;
    wifi.connect()?;
    while !wifi.is_up()? {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let ip_info = wifi.sta_netif().get_ip_info()?;
    println!("WiFi connected, IP: {}", ip_info.ip);

    let _sntp = EspSntp::new_default()?;
    loop {
        if _sntp.get_sync_status() == SyncStatus::Completed {
            println!("Time is synced");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let mut mdns = EspMdns::take()?;
    mdns.set_hostname("heaterz")?;
    mdns.set_instance_name("HeaterZ Automation")?;

    println!("Mdns published + fqdn heaterz.local");

    // discover_all_services(&mdns)?;
    let _plugs = scan_shelly_devices(&wifi)?;
    //println!("plugs = {:#?}", plugs);
    //std::fs::remove_file(FILE_PATH)?;

    let config = esp_idf_svc::http::server::Configuration::default();
    let mut server = EspHttpServer::new(&config)?;

    server.fn_handler("/", esp_idf_svc::http::Method::Get, |request| {
        handle_get_logs(request)
    })?;

    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let ip_info = wifi.sta_netif().get_ip_info()?;
        println!("IP: {:?}", ip_info.ip);
        let wattz = Wattz {
            source: "priza1".to_string(),
            wattz: 0,
            date: get_current_timestamp(),
        };
        println!("Wattz: {:?}", wattz);
        append_log(&wattz)?;
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Wattz {
    source: String,
    wattz: i16,
    date: u32,
}

fn append_log(record: &Wattz) -> anyhow::Result<()> {
    let _guard = FILE_LOCK.lock().unwrap();
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(FILE_PATH)?;

    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(file);
    wtr.serialize(record)?;
    wtr.flush()?;
    Ok(())
}

fn read_log() -> anyhow::Result<Vec<Wattz>> {
    let _guard = FILE_LOCK.lock().unwrap();
    if !Path::new(FILE_PATH).exists() {
        println!("File empty or not created.");
        return Ok(vec![]);
    }
    let mut output = Vec::<Wattz>::new();
    {
        let file = File::open(FILE_PATH)?;
        let buf_reader = BufReader::new(file);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .terminator(csv::Terminator::Any(b'\n'))
            .from_reader(buf_reader);
        for result in rdr.deserialize::<Wattz>() {
            let record = result?;
            output.push(record);
        }
    }
    std::fs::remove_file(FILE_PATH)?;
    Ok(output)
}

#[derive(Debug)]
struct ShellyDevice {
    ip: String,
    id: String,
    name: String,
    model: String,
}

use std::io::{Read, Write};

fn scan_shelly_devices(wifi: &EspWifi<'_>) -> anyhow::Result<Vec<ShellyDevice>> {
    let mut found = Vec::new();
    let ip_info = wifi.sta_netif().get_ip_info()?;
    let octets = ip_info.ip.octets();
    let subnet = format!("{}.{}.{}", octets[0], octets[1], octets[2]);

    println!("Scanning subnet {}.0/24 for Shellys...", subnet);

    let timeout = Duration::from_millis(120);

    for last_octet in 1..=254 {
        if last_octet == octets[3] || last_octet == 1 {
            continue;
        }

        let target_ip = format!("{}.{}", subnet, last_octet);
        let addr_str = format!("{}:80", target_ip);

        let Ok(addr) = addr_str.parse::<SocketAddr>() else {
            continue;
        };

        if let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
            let req = format!(
                "GET /rpc/Shelly.GetDeviceInfo HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                target_ip
            );

            if stream.write_all(req.as_bytes()).is_ok() {
                let mut buf = [0u8; 1024];
                if let Ok(len) = stream.read(&mut buf) {
                    let response = core::str::from_utf8(&buf[..len]).unwrap_or("");

                    if response.to_lowercase().contains("shellyplug") {
                        let id = extract_json_str(response, "\"id\":\"").unwrap_or_default();
                        let model = extract_json_str(response, "\"model\":\"").unwrap_or_default();
                        let name = extract_json_str(response, "\"name\":\"")
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| id.clone());

                        println!(
                            "Found plug:Name={}, IP={}, ID={}, Model={}",
                            name, target_ip, id, model
                        );
                        found.push(ShellyDevice {
                            ip: target_ip,
                            id,
                            name,
                            model,
                        });
                    }
                }
            }
        }
    }

    Ok(found)
}

fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let start = json.find(key)? + key.len();
    let end = json[start..].find('"')? + start;
    Some(json[start..end].to_string())
}

fn get_current_timestamp() -> u32 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32
}

fn handle_get_logs(request: Request<&mut EspHttpConnection>) -> Result<(), EspIOError> {
    let output = read_log().unwrap_or_else(|err| {
        println!("Error reading log: {}", err);
        vec![]
    });

    let json_string = serde_json::to_string(&output);

    let mut response = request.into_response(
        200,
        Some("OK"),
        &[
            ("Content-Type", "application/json"),
            ("Connection", "close"),
        ],
    )?;

    response.write(json_string.unwrap().as_bytes())?;
    Ok::<(), EspIOError>(())
}
