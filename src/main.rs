use esp_idf_svc::sys::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::server::{EspHttpServer};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration, EspWifi, };
use esp_idf_svc::mdns::EspMdns;
use esp_idf_svc::io::EspIOError;

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
            password: heapless::String::try_from("illuminati").unwrap(),
            ..Default::default()
        },
    ))?;
    wifi.start()?;
    wifi.connect()?;
    while !wifi.is_connected()? {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    println!("WiFi connected");

    let mut mdns = EspMdns::take()?;
    mdns.set_hostname("heaterz")?;
    mdns.set_instance_name("HeaterZ Automation")?;

    println!("Mdns connected + fqdn heaterz.local");
    // use std::io::Write;
    // let mut file = std::fs::OpenOptions::new()
    //     .create(true)
    //     .append(true)
    //     .open("/littlefs/test.txt")
    //     .unwrap();
    // writeln!(file, "\nxxxxxxxVVVVVVVVV").unwrap();
    //
    // std::fs::write("/littlefs/test.txt", "hello world").unwrap();
    // file.flush().unwrap();

    // std::fs::remove_file("/littlefs/test.txt").unwrap();

    println!(
        "exists = {}",
        std::path::Path::new("/littlefs/test.txt").exists()
    );

    let text = std::fs::read_to_string("/littlefs/test.txt").unwrap();

    println!("read: {}", text);

    let config = esp_idf_svc::http::server::Configuration::default();
    let mut server = EspHttpServer::new(&config)?;

    server.fn_handler("/", esp_idf_svc::http::Method::Get, |request| {
        // Формируем простой HTML-ответ
        let html = r#"Heaterz !!!!"#;
        let mut response = request.into_ok_response()?;
        response.write(html.as_bytes())?;

        Ok::<(), EspIOError>(())
    })?;


    loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let ip_info = wifi.sta_netif().get_ip_info()?;
        println!("IP: {:?}", ip_info.ip);
    }
}

// fn connect_wifi(
//     modem: Modem,
//     sysloop: EspSystemEventLoop,
//     nvs: EspDefaultNvsPartition,
// ) -> anyhow::Result<Box<EspWifi<'static>>> {
//     let mut wifi = Box::new(EspWifi::new(modem, sysloop, Some(nvs))?);
//
//     wifi.set_configuration(&Configuration::Client(ClientConfiguration {
//         ssid: heapless::String::try_from("USG Ishimura").unwrap(),
//         password: heapless::String::try_from("illuminati").unwrap(),
//         auth_method: AuthMethod::WPA2Personal,
//         ..Default::default()
//     }))?;
//
//     wifi.start()?;
//
//     println!("WiFi started");
//
//     wifi.connect()?;
//
//     println!("Connecting...");
//
//     while !wifi.is_connected()? {
//         std::thread::sleep(std::time::Duration::from_millis(500));
//     }
//
//     println!("WiFi connected");
//
//     let ip_info = wifi.sta_netif().get_ip_info()?;
//     println!("IP: {:?}", ip_info.ip);
//
//     Ok(wifi)
// }