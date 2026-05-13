use std::{
    io::{self, stdout},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{
    prelude::*,
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, BorderType},
};

use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind,
    ProcessesToUpdate, RefreshKind, System,
};

use nvml_wrapper::{
    enum_wrappers::device::TemperatureSensor,
    Nvml,
};

use crossbeam_channel::unbounded;

// =============================================
// КОНСТАНТЫ И СТИЛИ
// =============================================

const BORDER_COLOR: Color = Color::Rgb(48, 48, 48);
const TEXT_COLOR: Color = Color::Rgb(148, 148, 148);
const TITLE_COLOR: Color = Color::Rgb(112, 158, 158);

// =============================================
// СТРУКТУРА ДАННЫХ
// =============================================

#[derive(Clone)]
struct SystemData {
    cpu_name: String,
    cpu_usage: f32,
    cpu_cores_info: String,
    total_memory: u64,
    used_memory: u64,
    free_memory: u64,
    available_memory: u64,
    disks_text: String,
    gpu_name: String,
    gpu_usage: f64,
    gpu_temp: f64,
    gpu_power_text: String,
    gpu_mem_percent: f64,
    gpu_mem_text: String,
    network_text: String,
    download_speed: f64,
    upload_speed: f64,
    total_rx_mb: f64,
    total_tx_mb: f64,
    process_rows: Vec<Row<'static>>,
}

// =============================================
// ВСПОМОГАТЕЛЬНЫЕ ФУНКЦИИ
// =============================================

fn styled_block(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_COLOR))
        .style(Style::default().fg(TEXT_COLOR))
        .title_style(Style::default().fg(TITLE_COLOR))
}

fn bytes_to_gb(bytes: u64) -> f64 { bytes as f64 / 1024.0 / 1024.0 / 1024.0 }
fn bytes_to_mb(bytes: u64) -> f64 { bytes as f64 / 1024.0 / 1024.0 }

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_len.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

fn draw_bar(percent: f64, width: usize) -> String {
    let percent = percent.clamp(0.0, 100.0);
    let mut filled = ((percent / 100.0) * width as f64).round() as usize;
    if percent > 0.0 && filled == 0 { filled = 1; }
    let empty = width.saturating_sub(filled);
    format!("{}{}", "◼".repeat(filled), "◻".repeat(empty))
}

fn build_cpu_wave(history: &Vec<u64>, height: usize, width: usize) -> Vec<String> {
    let mut grid = vec![vec![' '; width]; height];
    let mid = height / 2;
    let start = if history.len() > width { history.len() - width } else { 0 };

    for (x, &value) in history[start..].iter().enumerate() {
        let amplitude = ((value as f64 / 100.0) * (height as f64 / 2.0)).round() as usize;
        grid[mid][x] = ':';
        for y in 1..=amplitude {
            if mid + y < height { grid[mid + y][x] = ':'; }
            if mid >= y { grid[mid - y][x] = ':'; }
        }
    }
    grid.into_iter().map(|row| row.into_iter().collect()).collect()
}

fn build_temp_graph(history: &Vec<u64>, height: usize, width: usize) -> Vec<String> {
    let mut grid = vec![vec![' '; width]; height];
    let start = if history.len() > width { history.len() - width } else { 0 };

    for (x, &value) in history[start..].iter().enumerate() {
        let percent = (value as f64 / 100.0).clamp(0.0, 1.0);
        let col_height = (percent * height as f64).round() as usize;
        for y in 0..col_height {
            if y < height {
                grid[height - 1 - y][x] = '.';
            }
        }
    }
    grid.into_iter().map(|row| row.into_iter().collect()).collect()
}

// =============================================
// BUILDERS
// =============================================

fn build_ram_text(used: f64, free: f64, avail: f64, total: u64, used_raw: u64, free_raw: u64, avail_raw: u64) -> String {
    let ram_p = if total > 0 { (used_raw as f64 / total as f64) * 100.0 } else { 0.0 };
    let free_p = if total > 0 { (free_raw as f64 / total as f64) * 100.0 } else { 0.0 };
    let avail_p = if total > 0 { (avail_raw as f64 / total as f64) * 100.0 } else { 0.0 };

    format!("\n Used: {:.2} GB\n {} {:.1}%\n\n Free: {:.2} GB\n {} {:.1}%\n\n Available: {:.2} GB\n {} {:.1}%",
            used, draw_bar(ram_p, 20), ram_p, free, draw_bar(free_p, 20), free_p, avail, draw_bar(avail_p, 20), avail_p)
}

fn build_disks_text(disks: &Disks) -> String {
    let mut text = String::new();
    for disk in disks.list() {
        let name = disk.mount_point().to_string_lossy().replace("\\", "");
        let total = disk.total_space();
        let avail = disk.available_space();
        let used = total.saturating_sub(avail);
        let percent = if total > 0 { (used as f64 / total as f64) * 100.0 } else { 0.0 };
        text.push_str(&format!(
            "\n {}: {:.1}% {} \n ({:.2} / {:.2} GB)\n\n",
            name.trim_end_matches(':'), percent, draw_bar(percent, 14), bytes_to_gb(used), bytes_to_gb(total)
        ));
    }
    text
}

fn build_network_text(total_rx_mb: f64, total_tx_mb: f64, download_speed: f64, upload_speed: f64) -> String {
    format!(
        "\n Download ↓{:>6.2} MB/s\n Total: {:.2} MB\n\n Upload ↑{:>6.2} MB/s\n Total: {:.2} MB",
        download_speed, total_rx_mb, upload_speed, total_tx_mb
    )
}

fn build_process_rows(system: &System) -> Vec<Row<'static>> {
    let mut processes: Vec<_> = system.processes().iter().collect();
    processes.sort_by(|a, b| b.1.memory().cmp(&a.1.memory()));

    processes.into_iter().take(25).map(|(pid, process)| {
        Row::new(vec![
            Cell::from(pid.to_string()),
            Cell::from(format!("{:.1}", process.cpu_usage())),
            Cell::from(format!("{:.1}", process.memory() as f64 / 1024.0 / 1024.0)),
            Cell::from(truncate_text(&process.name().to_string_lossy(), 26)),
            Cell::from(truncate_text(&process.exe().map_or("N/A".to_string(), |p| p.to_string_lossy().to_string()), 55)),
        ])
    }).collect()
}

fn build_gpu_text(gpu_name: &str, gpu_usage: f64, gpu_power_text: &str) -> String {
    if gpu_name == "No compatible device found" {
        return "NO COMPATIBLE DEVICE FOUND".to_string();
    }
    let gpu_bar = draw_bar(gpu_usage, 40);
    format!("GPU {} {:>5.1}%\n{}", gpu_bar, gpu_usage, gpu_power_text)
}

fn build_gpu_power_text(power_usage: u32, max_power: u32) -> (String, String) {
    if max_power == 0 {
        return ("PWR: N/A".to_string(), String::new());
    }
    let power_w = power_usage as f64 / 1000.0;
    let max_w = max_power as f64 / 1000.0;
    let percent = (power_w / max_w) * 100.0;
    let power_bar = draw_bar(percent, 40);
    (format!("PWR {} {:4.0}W", power_bar, power_w), power_bar)
}

// =============================================
// WORKER + COLLECT
// =============================================

fn data_collection_worker(tx: crossbeam_channel::Sender<SystemData>) {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    );
    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();
    let nvml = Nvml::init().ok();

    loop {
        system.refresh_cpu_all();
        system.refresh_memory();
        disks.refresh(true);
        networks.refresh(true);
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::everything());

        let data = collect_current_data(&system, &disks, &networks, &nvml);
        let _ = tx.send(data);

        std::thread::sleep(Duration::from_millis(200));
    }
}

fn collect_current_data(system: &System, disks: &Disks, networks: &Networks, nvml: &Option<Nvml>) -> SystemData {
    let cpu_name = system.cpus()[0].brand().to_string();
    let cpu_usage = system.global_cpu_usage();

    // Предварительно формируем строку с ядрами
    let mut cpu_cores_info = String::new();
    let temps = get_cpu_temps();
    let rows = 3;
    let cols = (system.cpus().len() + rows - 1) / rows;
    for r in 0..rows {
        for c in 0..cols {
            let index = c * rows + r;
            if index < system.cpus().len() {
                let cpu = &system.cpus()[index];
                let temp = if index < temps.len() { format!("{:.0}°C", temps[index]) } else { "N/A".to_string() };
                cpu_cores_info.push_str(&format!("C{:<1} {:>5.1}% {}", index, cpu.cpu_usage(), temp));
                if c + 1 < cols { cpu_cores_info.push_str(" | "); }
            }
        }
        cpu_cores_info.push('\n');
    }

    let total_memory = system.total_memory();
    let used_memory = system.used_memory();
    let free_memory = system.free_memory();
    let available_memory = system.available_memory();

    let disks_text = build_disks_text(disks);

    let mut gpu_usage_value = 0.0;
    let mut gpu_name = "No compatible device found".to_string();
    let mut gpu_memory_text = "Память GPU: Недоступно".to_string();
    let mut gpu_temp_value = 0.0;
    let mut gpu_mem_percent = 0.0;
    let mut gpu_power_text = "PWR: N/A".to_string();

    if let Some(nv) = nvml {
        if let Ok(device) = nv.device_by_index(0) {
            gpu_name = device.name().unwrap_or_else(|_| "Unknown GPU".to_string());
            if let Ok(util) = device.utilization_rates() { gpu_usage_value = util.gpu as f64; }
            if let Ok(mem) = device.memory_info() {
                let used_mb = bytes_to_mb(mem.used);
                let total_mb = bytes_to_mb(mem.total);
                gpu_mem_percent = (mem.used as f64 / mem.total as f64) * 100.0;
                gpu_memory_text = format!("Total: {:.1} GB | Used: {:.1} GB", total_mb / 1024.0, used_mb / 1024.0);
            }
            if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) { gpu_temp_value = temp as f64; }
            if let (Ok(power), Ok(max_power)) = (device.power_usage(), device.enforced_power_limit()) {
                let (text, _) = build_gpu_power_text(power, max_power);
                gpu_power_text = text;
            }
        }
    }

    let mut rx_bytes = 0u64; let mut tx_bytes = 0u64; let mut total_rx_bytes = 0u64; let mut total_tx_bytes = 0u64;
    for (_, network) in networks {
        rx_bytes += network.received();
        tx_bytes += network.transmitted();
        total_rx_bytes += network.total_received();
        total_tx_bytes += network.total_transmitted();
    }

    let rx_mb = bytes_to_mb(rx_bytes);
    let tx_mb = bytes_to_mb(tx_bytes);
    let total_rx_mb = bytes_to_mb(total_rx_bytes);
    let total_tx_mb = bytes_to_mb(total_tx_bytes);
    let interval_sec = 0.2;
    let download_speed = rx_mb / interval_sec;
    let upload_speed = tx_mb / interval_sec;

    let network_text = build_network_text(total_rx_mb, total_tx_mb, download_speed, upload_speed);
    let process_rows = build_process_rows(system);

    SystemData {
        cpu_name,
        cpu_usage,
        cpu_cores_info,
        total_memory,
        used_memory,
        free_memory,
        available_memory,
        disks_text,
        gpu_name,
        gpu_usage: gpu_usage_value,
        gpu_temp: gpu_temp_value,
        gpu_power_text,
        gpu_mem_percent,
        gpu_mem_text: gpu_memory_text,
        network_text,
        download_speed,
        upload_speed,
        total_rx_mb,
        total_tx_mb,
        process_rows,
    }
}

#[cfg(target_os = "windows")]
fn get_cpu_temps() -> Vec<f64> { Vec::new() }

#[cfg(target_os = "linux")]
fn get_cpu_temps() -> Vec<f64> { Vec::new() }

// =============================================
// APP
// =============================================

struct App {
    cpu_history: Vec<u64>,
    gpu_history: Vec<u64>,
    gpu_temp_history: Vec<u64>,
    gpu_mem_history: Vec<u64>,
    net_history: Vec<u64>,
    max_points: usize,
}

impl App {
    fn new(max_points: usize) -> Self {
        Self {
            cpu_history: Vec::new(),
            gpu_history: Vec::new(),
            gpu_temp_history: Vec::new(),
            gpu_mem_history: Vec::new(),
            net_history: Vec::new(),
            max_points,
        }
    }

    fn push_cpu(&mut self, value: f64) {
        let clamped = value.clamp(0.0, 100.0) as u64;
        self.cpu_history.push(clamped);
        if self.cpu_history.len() > self.max_points { self.cpu_history.remove(0); }
    }

    fn push_gpu(&mut self, value: f64) {
        let clamped = value.clamp(0.0, 100.0) as u64;
        self.gpu_history.push(clamped);
        if self.gpu_history.len() > self.max_points { self.gpu_history.remove(0); }
    }

    fn push_gpu_temp(&mut self, value: f64) {
        self.gpu_temp_history.push(value as u64);
        if self.gpu_temp_history.len() > self.max_points { self.gpu_temp_history.remove(0); }
    }

    fn push_gpu_mem(&mut self, percent: f64) {
        let val = percent.clamp(0.0, 100.0) as u64;
        self.gpu_mem_history.push(val);
        if self.gpu_mem_history.len() > self.max_points { self.gpu_mem_history.remove(0); }
    }

    fn push_net(&mut self, value: f64) {
        let clamped = value.clamp(0.0, 100.0) as u64;
        self.net_history.push(clamped);
        if self.net_history.len() > self.max_points { self.net_history.remove(0); }
    }
}

// =============================================
// MAIN
// =============================================

fn main() -> io::Result<()> {
    let (tx, rx) = unbounded::<SystemData>();

    std::thread::spawn(move || data_collection_worker(tx));

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(120);
    let mut current_data: Option<SystemData> = None;

    loop {
        if let Ok(data) = rx.try_recv() {
            current_data = Some(data.clone());
            app.push_cpu(data.cpu_usage as f64);
            app.push_gpu(data.gpu_usage);
            app.push_gpu_temp(data.gpu_temp);
            app.push_gpu_mem(data.gpu_mem_percent);
            let net_activity = ((data.download_speed + data.upload_speed) / 1.0) * 100.0;
            app.push_net(net_activity);
        }

        terminal.draw(|frame| {
            let area = frame.area();

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Percentage(20), Constraint::Percentage(26), Constraint::Percentage(52)])
                .split(area);

            let cpu_block = styled_block(" CPU ");
            let gpu_block = styled_block(" GPU ");

            frame.render_widget(cpu_block.clone(), vertical[0]);
            frame.render_widget(gpu_block.clone(), vertical[1]);

            let cpu_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Length(2), Constraint::Percentage(40)])
                .split(cpu_block.inner(vertical[0]));

            let gpu_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Length(2), Constraint::Percentage(40)])
                .split(gpu_block.inner(vertical[1]));

            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
                .split(vertical[2]);

            let left_column = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(bottom[0]);

            let left_top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(left_column[0]);

            if let Some(data) = &current_data {
                // CPU
                frame.render_widget(Paragraph::new(build_cpu_wave(&app.cpu_history, cpu_split[0].height as usize, cpu_split[0].width as usize).join("\n")), cpu_split[0]);

                let cpu_info_block = styled_block(format!(" {} ", data.cpu_name));
                frame.render_widget(cpu_info_block.clone(), cpu_split[2]);

                let cpu_bar = draw_bar(data.cpu_usage as f64, 40);

                let cpu_text = format!("\nCPU {} {:>5.1}%\n\n{}", cpu_bar, data.cpu_usage, data.cpu_cores_info);

                frame.render_widget(Paragraph::new(cpu_text), cpu_info_block.inner(cpu_split[2]));

                // GPU
                frame.render_widget(Paragraph::new(build_cpu_wave(&app.gpu_history, gpu_split[0].height as usize, gpu_split[0].width as usize).join("\n")), gpu_split[0]);

                let gpu_right = Layout::default() .direction(Direction::Vertical) .constraints([ Constraint::Length(5), Constraint::Min(5), ]) .split(gpu_split[2]);

                let gpu_text = build_gpu_text( &data.gpu_name, data.gpu_usage, &data.gpu_power_text );

                frame.render_widget( Paragraph::new(gpu_text) .block(styled_block(format!(" {} ", data.gpu_name))), gpu_right[0] );

                // TEMP + VRAM
                let gpu_bottom = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ])
                    .split(gpu_right[1]);

                frame.render_widget(
                    Paragraph::new(
                        build_temp_graph(
                            &app.gpu_temp_history,
                            gpu_bottom[0].height.saturating_sub(2) as usize,
                            gpu_bottom[0].width.saturating_sub(2) as usize
                        ).join("\n")
                    )
                        .block(styled_block(format!(" TEMP — {:.0}°C ", data.gpu_temp))),
                    gpu_bottom[0]
                );

                let vram_bar = draw_bar(data.gpu_mem_percent, 20);
                frame.render_widget(
                    Paragraph::new(format!(
                        "{}\n\n{} {:>5.1}%",
                        data.gpu_mem_text,
                        vram_bar,
                        data.gpu_mem_percent
                    ))
                        .block(styled_block(" VRAM ")),
                    gpu_bottom[1] );

                // RAM + DISKS
                let ram_text = build_ram_text(
                    bytes_to_gb(data.used_memory), bytes_to_gb(data.free_memory), bytes_to_gb(data.available_memory),
                    data.total_memory, data.used_memory, data.free_memory, data.available_memory
                );
                frame.render_widget(Paragraph::new(ram_text).block(styled_block(format!(" RAM — {:.2}GB ", bytes_to_gb(data.total_memory)))), left_top[0]);
                frame.render_widget(Paragraph::new(data.disks_text.clone()).block(styled_block(" DISKS ")), left_top[1]);

                // NETWORK
                let network_block = styled_block(" NETWORK ");
                let network_inner = network_block.inner(left_column[1]);
                frame.render_widget(network_block.clone(), left_column[1]);
                let network_split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(network_inner);

                frame.render_widget(Paragraph::new(build_cpu_wave(&app.net_history, network_split[0].height as usize, network_split[0].width as usize).join("\n")), network_split[0]);
                frame.render_widget(Paragraph::new(data.network_text.clone()).block(styled_block(" INFO ")), network_split[1]);

                // PROCESSES
                let processes_widget = Table::new(
                    data.process_rows.clone(),
                    [Constraint::Length(9), Constraint::Length(8), Constraint::Length(11), Constraint::Length(24), Constraint::Min(16)],
                )
                    .header(Row::new(vec!["PID", "CPU%", "RAM(MB)", "NAME", "PATH"]))
                    .column_spacing(3)
                    .block(styled_block(" PROCESSES "));

                frame.render_widget(processes_widget, bottom[1]);
            }
        })?;

        if event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}