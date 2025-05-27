use actix_web::{get, App, HttpResponse, HttpServer, HttpRequest, Responder, web};
use serde::Serialize;
use std::process::Command;
use get_if_addrs::get_if_addrs;
use sysinfo::System;
use std::env;
use dotenv::dotenv;

#[derive(Serialize)]
struct TempData {
    motherboard_temp: Option<f32>,
    cpu_temp: Option<f32>,
    gpu_temp: Option<f32>,
}

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
    temps: TempData,
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

fn get_all_temps() -> TempData {
    let output = Command::new("sensors").output().ok();
    let stdout = output.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();

    let mut current_chip: Option<String> = None;
    let mut motherboard_temp = None;
    let mut cpu_temp = None;
    let mut gpu_temp = None;

    for line in stdout.lines() {
        if !line.starts_with(' ') && !line.is_empty() && !line.contains(':') {
            current_chip = Some(line.to_string());
        }

        if let Some(chip) = &current_chip {
            let lower = chip.to_lowercase();

            if (lower.contains("asus") || lower.contains("acpitz")) && line.trim().to_lowercase().contains("temp1:") {
                motherboard_temp = parse_temp_line(line);
            } else if lower.contains("k10temp") && line.trim().to_lowercase().contains("temp1:") {
                cpu_temp = parse_temp_line(line);
            } else if lower.contains("amdgpu") && line.trim().to_lowercase().contains("edge:") {
                gpu_temp = parse_temp_line(line);
            }
        }
    }

    TempData {
        motherboard_temp,
        cpu_temp,
        gpu_temp,
    }
}

fn parse_temp_line(line: &str) -> Option<f32> {
    for word in line.split_whitespace() {
        if word.contains("°C") {
            let clean = word.trim_matches(|c| c == '+' || c == '°' || c == 'C');
            return clean.parse::<f32>().ok();
        }
    }
    None
}

// fn validate_token(req: &HttpRequest) -> bool {
//     const TOKEN: &str = "your-secret-token-here";

//     if let Some(auth_header) = req.headers().get("Authorization") {
//         if let Ok(auth_str) = auth_header.to_str() {
//             return auth_str == format!("Bearer {}", TOKEN);
//         }
//     }

//     false
// }

#[get("/status")]
async fn status(req: HttpRequest, token: web::Data<String>) -> impl Responder {
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str == format!("Bearer {}", token.get_ref()) {
                // Authorized — continue with your existing logic

                let mut sys = System::new_all();
                sys.refresh_all();

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

                let all_temps = get_all_temps();

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
                        temps: all_temps,
                    },
                    network: NetworkData {
                        public_ip,
                        ping_ms,
                        speed_download_mbps,
                        speed_upload_mbps,
                        interfaces,
                    },
                };

                return HttpResponse::Ok().json(response);
            }
        }
    }

    HttpResponse::Unauthorized().body("Unauthorized")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let token = env::var("BEARER_TOKEN").unwrap_or_else(|_| "default-token".to_string());

    println!("🚀 Server running on http://localhost:{}", port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(token.clone()))
            .service(status)
        })
        .bind(format!("0.0.0.0:{}", port))?
        .run()
        .await
}

