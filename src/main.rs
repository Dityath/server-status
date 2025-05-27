use actix_web::{get, App, HttpResponse, HttpServer, Responder};
use serde::Serialize;
use std::process::Command;
use get_if_addrs::get_if_addrs;
use sysinfo::{Components, System};

#[derive(Serialize)]
struct ServerData {
    server_name: Option<String>,
    server_cpu: String,
    server_os: Option<String>,
}

#[derive(Serialize)]
struct UsageData {
    cpu_percentage: f32,
    memory: f32,
    total_memory: f32,
    memory_percentage: f32,
    cpu_temp: Option<f32>,
}

#[derive(Serialize)]
struct NetworkInterface {
    name: String,
    ip: String,
}

#[derive(Serialize)]
struct NetworkData {
    public_ip: String,
    ping_ms: Option<f64>,
    speed_download_mbps: Option<f64>,
    speed_upload_mbps: Option<f64>,
    interfaces: Vec<NetworkInterface>,
}

#[derive(Serialize)]
struct StatusResponse {
    server_status: String,
    server_uptime: String,
    server_data: ServerData,
    data: UsageData,
    network: NetworkData,
}

fn get_ping_ms() -> Option<f64> {
    let output = Command::new("ping")
        .arg("-c")
        .arg("1")
        .arg("8.8.8.8")
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines()
        .find(|line| line.contains("time="))
        .and_then(|line| {
            line.split("time=").nth(1)?.split(' ').next()?.parse::<f64>().ok()
        })
}

fn get_speedtest() -> Option<(f64, f64)> {
    let output = Command::new("speedtest-cli")
        .arg("--simple")
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut download = None;
    let mut upload = None;

    for line in stdout.lines() {
        if line.starts_with("Download:") {
            download = line.split_whitespace().nth(1)?.parse::<f64>().ok();
        } else if line.starts_with("Upload:") {
            upload = line.split_whitespace().nth(1)?.parse::<f64>().ok();
        }
    }

    match (download, upload) {
        (Some(d), Some(u)) => Some((d, u)),
        _ => None,
    }
}

fn get_network_interfaces() -> Vec<NetworkInterface> {
    get_if_addrs().unwrap_or_default()
        .into_iter()
        .map(|iface| NetworkInterface {
            name: iface.name.clone(),
            ip: iface.ip().to_string(),
        })
        .collect()
}

fn get_from_sensors() -> Option<f32> {
    let output = Command::new("sensors").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.to_lowercase().contains("package id 0") {
            // Find something like: +39.0°C
            if let Some(temp_str) = line.split_whitespace().find(|s| s.contains("°C")) {
                let cleaned = temp_str.trim_start_matches('+').trim_end_matches("°C");
                if let Ok(temp) = cleaned.parse::<f32>() {
                    return Some(temp);
                }
            }
        }
    }

    None
}

#[get("/status")]
async fn status() -> impl Responder {
    let mut sys = System::new_all();
    sys.refresh_all();

    // Call uptime as associated function, NOT method
    let uptime_secs = sysinfo::System::uptime();
    let uptime = format!(
        "{}h {}m {}s",
        uptime_secs / 3600,
        (uptime_secs % 3600) / 60,
        uptime_secs % 60
    );

    let cpu_name = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
    let cpu_percentage = sys.global_cpu_info().cpu_usage();

    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();
    let memory_percentage = (used_memory as f32 / total_memory as f32) * 100.0;

    let public_ip = ureq::get("https://api.ipify.org")
        .call()
        .ok()
        .and_then(|res| res.into_string().ok())
        .unwrap_or_else(|| "Unavailable".to_string());

    let ping_ms = get_ping_ms();

    let (speed_download_mbps, speed_upload_mbps) = match get_speedtest() {
        Some((d, u)) => (Some(d), Some(u)),
        None => (None, None),
    };

    let interfaces = get_network_interfaces();

    let mut components = Components::new();
    components.refresh();

    let cpu_temp = components
        .iter()
        .find(|c| c.label().to_lowercase().contains("cpu"))
        .map(|c| c.temperature())
        .or_else(get_from_sensors);


    let response = StatusResponse {
        server_status: "online".to_string(),
        server_uptime: uptime,
        server_data: ServerData {
            server_name: sysinfo::System::host_name(),
            server_cpu: cpu_name,
            server_os: sysinfo::System::name(),
        },
        data: UsageData {
            cpu_percentage,
            memory: used_memory as f32 / (1024.0 * 1024.0 * 1024.0),
            total_memory: total_memory as f32 / (1024.0 * 1024.0 * 1024.0),
            memory_percentage,
            cpu_temp,
        },
        network: NetworkData {
            public_ip,
            ping_ms,
            speed_download_mbps,
            speed_upload_mbps,
            interfaces,
        },
    };

    HttpResponse::Ok().json(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("🚀 Server running on http://localhost:8080");

    HttpServer::new(|| App::new().service(status))
        .bind("0.0.0.0:8080")?
        .run()
        .await
}

