use esp_idf_svc::sys::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::server::{EspHttpServer};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration, EspWifi, };
use esp_idf_svc::mdns::EspMdns;
use esp_idf_svc::io::EspIOError;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::BufReader;
use std::path::Path;
use std::time::SystemTime;

static FILE_PATH: &str = "/littlefs/log.csv";

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

        if rc != ESP_OK as i32 {
            panic!("LittleFS mount failed: {}", rc);
        }
    }

    println!("Storage mounted!");

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;
    let mut wifi = EspWifi::new(
        peripherals.modem,
        sysloop,
        Some(nvs),
    )?;
    wifi.set_configuration(&Configuration::Client(
        ClientConfiguration {
            ssid: heapless::String::try_from("USG Ishimura").unwrap(),
            password: heapless::String::try_from("pwd").unwrap(),
            ..Default::default()
        },
    ))?;
    wifi.start()?;
    wifi.connect()?;
    while !wifi.is_connected()? {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    println!("WiFi connected");

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

    std::fs::remove_file(FILE_PATH).unwrap();

    let config = esp_idf_svc::http::server::Configuration::default();
    let mut server = EspHttpServer::new(&config)?;

    server.fn_handler("/", esp_idf_svc::http::Method::Get, |request| {
        if let Err(e) = read_log() {
            println!("Error reading log file : {:?}", e);
        }

        // let text = std::fs::read_to_string(FILE_PATH).unwrap();
        // println!("read file: {}", text);
        let html = r#"Heaterz !!!!"#;
        let mut response = request.into_ok_response()?;
        response.write(html.as_bytes())?;
        Ok::<(), EspIOError>(())
    })?;


    loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let ip_info = wifi.sta_netif().get_ip_info()?;
        println!("IP: {:?}", ip_info.ip);
        let wattz = Wattz{
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

fn read_log() -> anyhow::Result<()> {
    let file = File::open(FILE_PATH)?;
    let buf_reader = BufReader::new(file);
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        //.flexible(true)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_reader(buf_reader);
    for result in rdr.deserialize() {
        let record: Wattz = result?;
        println!("{:?}", record);
    }
    Ok(())
}

fn get_current_timestamp() -> u32 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32
}