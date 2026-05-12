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
    widgets::{Block, Borders, Paragraph, Cell, Row, Table},
    style::{Color, Style},
    widgets::BorderType,
};

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, Networks};

use nvml_wrapper::{
    enum_wrappers::device::TemperatureSensor,
    Nvml,
};

const BORDER_COLOR: Color = Color::Rgb(48, 48, 48);
const TEXT_COLOR: Color = Color::Rgb(148, 148, 148);
const TITLE_COLOR: Color = Color::Rgb(112, 158, 158);

fn styled_block(title: impl Into<ratatui::text::Line<'static>>) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_COLOR))
        .style(Style::default().fg(TEXT_COLOR))
        .title_style(Style::default().fg(TITLE_COLOR))
}

fn bytes_to_gb(bytes: u64) -> f64{
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn bytes_to_mb(bytes: u64) -> f64{
    bytes as f64 / 1024.0 / 1024.0
}

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

    if percent > 0.0 && filled == 0 {
        filled = 1;
    }

    let empty = width.saturating_sub(filled);
    format!("{}{}", "◼".repeat(filled), "◻".repeat(empty))
}

fn build_cpu_wave(history: &Vec<u64>, height: usize, width: usize) -> Vec<String> {
    let mut grid = vec![vec![' '; width]; height];

    let mid = height / 2;

    let start = if history.len() > width {
        history.len() - width
    } else {
        0
    };

    for (x, value) in history[start..].iter().enumerate() {
        let amplitude = ((*value as f64 / 100.0) * (height as f64 / 2.0)).round() as usize;

        // центр — ВСЕГДА одна линия
        grid[mid][x] = ':';

        // расширение вверх и вниз
        for y in 1..=amplitude {
            if mid + y < height {
                grid[mid + y][x] = ':';
            }
            if mid >= y {
                grid[mid - y][x] = ':';
            }
        }
    }

    grid.into_iter()
        .map(|row| row.into_iter().collect())
        .collect()
}

fn build_temp_graph(history: &Vec<u64>, height: usize, width: usize) -> Vec<String> {
    let mut grid = vec![vec![' '; width]; height];

    let start = if history.len() > width {
        history.len() - width
    } else {
        0
    };

    for (x, value) in history[start..].iter().enumerate() {
        let percent = (*value as f64 / 100.0).clamp(0.0, 1.0);

        let col_height = (percent * height as f64).round() as usize;

        for y in 0..col_height {
            if y < height {
                // рисуем СНИЗУ вверх
                grid[height - 1 - y][x] = '.';
            }
        }
    }

    grid.into_iter()
        .map(|row| row.into_iter().collect())
        .collect()
}

fn build_ram_text(
    used_memory_gb: f64,
    free_memory_gb: f64,
    available_memory_gb: f64,
    total_memory: u64,
    used_memory: u64,
    free_memory: u64,
    available_memory: u64,
) -> String {
    let ram_percent = (used_memory as f64 / total_memory as f64) * 100.0;
    let free_percent = (free_memory as f64 / total_memory as f64) * 100.0;
    let avail_percent = (available_memory as f64 / total_memory as f64) * 100.0;

    let ram_bar = draw_bar(ram_percent, 20);
    let free_bar = draw_bar(free_percent, 20);
    let avail_bar = draw_bar(avail_percent, 20);

    format!(
        "\n Used: {:.2} GB\n {} {:.1}%\n\n Free: {:.2} GB\n {} {:.1}%\n\n Available: {:.2} GB\n {} {:.1}%",
        used_memory_gb,
        ram_bar,
        ram_percent,
        free_memory_gb,
        free_bar,
        free_percent,
        available_memory_gb,
        avail_bar,
        avail_percent,
    )
}

fn build_disks_text(disks: &Disks) -> String {
    let mut disks_text = String::new();

    for disk in disks.list() {
        let name = disk.mount_point().to_string_lossy().replace("\\", "");
        let total_space = disk.total_space();
        let available_space = disk.available_space();
        let used_space = total_space - available_space;

        let total_space_gb = bytes_to_gb(total_space);
        let used_space_gb = bytes_to_gb(used_space);

        let disk_percent = if total_space > 0 {
            (used_space as f64 / total_space as f64) * 100.0
        } else {
            0.0
        };

        let clean_name = name.trim_end_matches(':');
        let disk_bar = draw_bar(disk_percent, 14);

        disks_text.push_str(&format!(
            "\n {}: {:.1}% {} \n ({:.2} / {:.2} GB)\n\n",
            clean_name,
            disk_percent,
            disk_bar,
            used_space_gb,
            total_space_gb
        ));
    }

    disks_text
}

fn build_network_text(
    total_rx_mb: f64,
    total_tx_mb: f64,
    download_speed: f64,
    upload_speed: f64,
) -> String {
    format!(
        "\n Download ↓{:>6.2} MB/s\n Total: {:.2} MB\n\n Upload ↑{:>6.2} MB/s\n Total: {:.2} MB",
        download_speed,
        total_rx_mb,
        upload_speed,
        total_tx_mb
    )
}

fn build_cpu_text(
    cpu_usage: f32,
    cpus: &[sysinfo::Cpu],
    temps: &[f64],
) -> String {
    let cpu_bar = draw_bar(cpu_usage as f64, 30);

    let mut cpu_text = format!(
        "CPU {} {:>5.1}%\n\n",
        cpu_bar, cpu_usage
    );

    let rows = 3;
    let cols = (cpus.len() + rows - 1) / rows;

    for r in 0..rows {
        let mut line = String::new();

        for c in 0..cols {
            let index = c * rows + r;

            if index < cpus.len() {
                let cpu = &cpus[index];

                let temp_text = if index < temps.len() {
                    format!("{:.0}°C", temps[index])
                } else {
                    "N/A".to_string()
                };

                let core_text = format!(
                    "C{:<1} {:>5.1}% {}",
                    index,
                    cpu.cpu_usage(),
                    temp_text
                );

                line.push_str(&core_text);

                if c + 1 < cols {
                    line.push_str(" | ");
                }
            }
        }

        cpu_text.push_str(&line);
        cpu_text.push('\n');
    }

    cpu_text
}

fn build_process_rows(system: &System) -> Vec<Row> {
    let mut processes: Vec<_> = system.processes().iter().collect();

    processes.sort_by(|a, b| b.1.memory().cmp(&a.1.memory()));

    processes
        .into_iter()
        .take(25)
        .map(|(pid, process)| {
            let cpu = format!("{:.1}", process.cpu_usage());

            let memory_mb = format!(
                "{:.1}",
                process.memory() as f64 / 1024.0 / 1024.0
            );

            let name = truncate_text(
                &process.name().to_string_lossy(),
                26
            );

            let path = process
                .exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or("N/A".to_string());

            let path = truncate_text(&path, 55);

            Row::new(vec![
                Cell::from(pid.to_string()),
                Cell::from(cpu),
                Cell::from(memory_mb),
                Cell::from(name),
                Cell::from(path),
            ])
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn get_cpu_temps() -> Vec<f64> {
    use std::fs;

    let mut temps = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let path = entry.path();

            for i in 1..=10 {
                let temp_path = path.join(format!("temp{}_input", i));

                if let Ok(content) = fs::read_to_string(&temp_path) {
                    if let Ok(value) = content.trim().parse::<f64>() {
                        temps.push(value / 1000.0);
                    }
                }
            }
        }
    }

    temps
}

#[cfg(target_os = "windows")]
fn get_cpu_temps() -> Vec<f64> {
    Vec::new() // fallback
}
struct App{
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

        if self.cpu_history.len() > self.max_points {
            self.cpu_history.remove(0);
        }
    }

    fn push_gpu(&mut self, value: f64) {
        let clamped = value.clamp(0.0, 100.0) as u64;
        self.gpu_history.push(clamped);

        if self.gpu_history.len() > self.max_points {
            self.gpu_history.remove(0);
        }
    }

    fn push_gpu_temp(&mut self, value: f64) {
        let val = value as u64;
        self.gpu_temp_history.push(val);

        if self.gpu_temp_history.len() > self.max_points {
            self.gpu_temp_history.remove(0);
        }
    }

    fn push_gpu_mem(&mut self, percent: f64) {
        let val = percent.clamp(0.0, 100.0) as u64;
        self.gpu_mem_history.push(val);

        if self.gpu_mem_history.len() > self.max_points {
            self.gpu_mem_history.remove(0);
        }
    }

    fn push_net(&mut self, value: f64) {
        let clamped = value.clamp(0.0, 100.0) as u64;
        self.net_history.push(clamped);

        if self.net_history.len() > self.max_points {
            self.net_history.remove(0);
        }
    }
}

fn main() -> io::Result<()> {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    );

    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();

    let nvml = Nvml::init().ok();
    let mut app = App::new(120);

    system.refresh_cpu_all();
    system.refresh_memory();

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    loop {
        system.refresh_cpu_all();
        system.refresh_memory();
        disks.refresh(true);
        networks.refresh(true);
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );

        let cpu_name = system.cpus()[0].brand().to_string();
        let cpu_usage = system.global_cpu_usage();
        let cpus = system.cpus();
        let temps = get_cpu_temps();

        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let free_memory = system.free_memory();
        let available_memory = system.available_memory();

        let total_memory_gb = bytes_to_gb(total_memory);
        let used_memory_gb = bytes_to_gb(used_memory);
        let free_memory_gb = bytes_to_gb(free_memory);
        let available_memory_gb = bytes_to_gb(available_memory);

        let cpu_text = build_cpu_text(cpu_usage, cpus, &temps);

        let mut gpu_usage_value = 0.0;
        let mut gpu_name = "No compatible device found".to_string();
        let mut gpu_memory_text = "Память GPU: Недоступно".to_string();
        let mut gpu_temp_text = "Температура: Недоступно".to_string();

        let mut gpu_temp_value = 0.0;
        let mut gpu_mem_percent = 0.0;

        if let Some(nvml) = &nvml {
            if let Ok(device) = nvml.device_by_index(0) {
                gpu_name = device
                    .name()
                    .unwrap_or_else(|_| "Unknown GPU".to_string());

                if let Ok(util) = device.utilization_rates() {
                    gpu_usage_value = util.gpu as f64;
                }

                if let Ok(mem) = device.memory_info() {
                    let used_mb = bytes_to_mb(mem.used);
                    let total_mb = bytes_to_mb(mem.total);

                    gpu_mem_percent = (mem.used as f64 / mem.total as f64) * 100.0;

                    gpu_memory_text = format!(
                        "Total: {:.1} GB | Used: {:.1} GB",
                        total_mb / 1024.0,
                        used_mb / 1024.0
                    );
                }

                if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                    gpu_temp_value = temp as f64;
                    gpu_temp_text = format!("Температура: {}°C", temp);
                }
            }
        }

        app.push_cpu(cpu_usage as f64);
        app.push_gpu(gpu_usage_value);
        app.push_gpu_temp(gpu_temp_value);
        app.push_gpu_mem(gpu_mem_percent);

        let gpu_bar = draw_bar(gpu_usage_value, 40);

        let mut gpu_power_text = "PWR: N/A".to_string();
        let mut gpu_power_bar = String::new();
        if let Some(nvml) = &nvml {
            if let Ok(device) = nvml.device_by_index(0) {

                if let (Ok(power), Ok(max_power)) = (
                    device.power_usage(),
                    device.enforced_power_limit()
                    ) {
                    let power_w = power as f64 / 1000.0;
                    let max_w = max_power as f64 / 1000.0;

                    let percent = (power_w / max_w) * 100.0;

                    gpu_power_bar = draw_bar(percent, 40);

                    gpu_power_text = format! (
                        "PWR {} {:4.0}W",
                        gpu_power_bar,
                        power_w
                    );
                }
            }
        }

        let gpu_text = if gpu_name == "No compatible device found" {
            "NO COMPATIBLE DEVICE FOUND".to_string()
        } else {
            format!(
                "GPU {} {:>5.1}%\n{}",
                gpu_bar, gpu_usage_value, gpu_power_text
            )
        };

        let ram_text = build_ram_text(
            used_memory_gb,
            free_memory_gb,
            available_memory_gb,
            total_memory,
            used_memory,
            free_memory,
            available_memory,
        );

        let disks_text = build_disks_text(&disks);

        let mut rx_bytes = 0u64;
        let mut tx_bytes = 0u64;
        let mut total_rx_bytes = 0u64;
        let mut total_tx_bytes = 0u64;

        for (_interface_name, network) in &networks {
            rx_bytes += network.received();
            tx_bytes += network.transmitted();
            total_rx_bytes += network.total_received();
            total_tx_bytes += network.total_transmitted();
        }

        let rx_mb = bytes_to_mb(rx_bytes);
        let tx_mb = bytes_to_mb(tx_bytes);
        let total_rx_mb = bytes_to_mb(total_rx_bytes);
        let total_tx_mb = bytes_to_mb(total_tx_bytes);
        // 200 мс = 0.2 секунды, потому что у тебя poll стоит на 200 ms
        let interval_sec = 0.2;
        let download_speed = rx_mb / interval_sec;
        let upload_speed = tx_mb / interval_sec;
        // 1 MB/s считаем как 100% активности для графика
        let net_activity = ((download_speed + upload_speed) / 1.0) * 100.0;
        app.push_net(net_activity);

        let network_text = build_network_text(
            total_rx_mb,
            total_tx_mb,
            download_speed,
            upload_speed,
        );

        let process_rows = build_process_rows(&system);

        terminal.draw(|frame| {
            let area = frame.area();

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Percentage(23), //CPU
                    Constraint::Percentage(25), //GPU
                    Constraint::Percentage(52),
                ])
                .split(area);

            let cpu_block = styled_block(" CPU ");
            let gpu_block = styled_block(" GPU ");

            let cpu_inner = cpu_block.inner(vertical[0]);

            let cpu_compact = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(8),
                    Constraint::Min(0),
                ])
                .split(cpu_inner);

            let gpu_inner = gpu_block.inner(vertical[1]);

            frame.render_widget(cpu_block, vertical[0]);
            frame.render_widget(gpu_block, vertical[1]);

            let cpu_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(60),
                    Constraint::Length(2),
                    Constraint::Percentage(40),
                ])
                .split(cpu_compact[1]);

            let gpu_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(63),
                    Constraint::Length(2),
                    Constraint::Percentage(37),
                ])
                .split(gpu_inner);

            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(42),
                    Constraint::Percentage(58),
                ])
                .split(vertical[2]);

            let left_column = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(55), // RAM + DISKS
                    Constraint::Percentage(45), // NETWORK
                ])
                .split(bottom[0]);

            let left_top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(left_column[0]);

            let cpu_height = cpu_split[0].height as usize;

            let cpu_wave_lines = build_cpu_wave(&app.cpu_history, cpu_height, cpu_split[0].width as usize);

            let cpu_wave_text = cpu_wave_lines.join("\n");

            let cpu_chart = Paragraph::new(cpu_wave_text);

            let gpu_height = gpu_split[0].height as usize;

            let gpu_wave_lines = build_cpu_wave(
                &app.gpu_history,
                gpu_height,
                gpu_split[0].width as usize
            );

            let gpu_wave_text = gpu_wave_lines.join("\n");

            let gpu_chart = Paragraph::new(gpu_wave_text);

            let gpu_right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(5), // верхний блок: основная инфа
                    Constraint::Min(5),    // нижняя часть: temp + memory
                ])
                .split(gpu_split[2]);

            let gpu_bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(gpu_right[1]);


            let cpu_info_block = styled_block(format!(" {} ", cpu_name));

            let cpu_info_inner = cpu_info_block.inner(cpu_split[2]);

            let cpu_info = Paragraph::new(cpu_text);

            let gpu_info_block = styled_block(format!(" {} ", gpu_name));

            let gpu_info_inner = gpu_info_block.inner(gpu_right[0]);

            let gpu_info = Paragraph::new(gpu_text);

            let temp_graph_lines = build_temp_graph(
                &app.gpu_temp_history,
                gpu_bottom[0].height.saturating_sub(2) as usize,
                gpu_bottom[0].width.saturating_sub(2) as usize,
            );

            let gpu_temp_widget = Paragraph::new(temp_graph_lines.join("\n"))
                .block(
                    styled_block(format!(" TEMP — {}°C ", gpu_temp_value as u64))
                );

            let vram_bar = draw_bar(gpu_mem_percent, 20);

            let gpu_mem_widget = Paragraph::new(format!(
                "{}\n\n{} {:>5.1}%",
                gpu_memory_text,
                vram_bar,
                gpu_mem_percent
            ))
                .block(styled_block(" VRAM "));

            let ram_widget = Paragraph::new(ram_text)
                .block(
                    styled_block(format!(" RAM — {:>5.2}GB ", total_memory_gb))
                );

            let disks_widget = Paragraph::new(disks_text)
                .block(
                    styled_block(" DISKS ")
                );

            let network_block = styled_block(" NETWORK ");

            let network_inner = network_block.inner(left_column[1]);

            let network_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(60),
                    Constraint::Percentage(40),
                ])
                .split(network_inner);

            let net_wave_lines = build_cpu_wave(
                &app.net_history,
                network_split[0].height as usize,
                network_split[0].width as usize,
            );

            let net_chart = Paragraph::new(net_wave_lines.join("\n"));

            let net_info = Paragraph::new(network_text)
                .block(styled_block(" INFO "));

            let processes_widget = Table::new(
                process_rows,
                [
                    Constraint::Length(9),
                    Constraint::Length(8),
                    Constraint::Length(11),
                    Constraint::Length(24),
                    Constraint::Min(16),
                ],
            )
                .header(Row::new(vec!["PID", "CPU%", "RAM(MB)", "NAME", "PATH"]))
                .column_spacing(3)
                .block(styled_block(" PROCESSES "));

            frame.render_widget(cpu_chart, cpu_split[0]);

            frame.render_widget(cpu_info_block, cpu_split[2]);
            frame.render_widget(cpu_info, cpu_info_inner);

            frame.render_widget(gpu_chart, gpu_split[0]);

            frame.render_widget(gpu_info_block, gpu_right[0]);
            frame.render_widget(gpu_info, gpu_info_inner);

            frame.render_widget(gpu_temp_widget, gpu_bottom[0]);
            frame.render_widget(gpu_mem_widget, gpu_bottom[1]);


            frame.render_widget(ram_widget, left_top[0]);
            frame.render_widget(disks_widget, left_top[1]);

            frame.render_widget(network_block, left_column[1]);
            frame.render_widget(net_chart, network_split[0]);
            frame.render_widget(net_info, network_split[1]);

            frame.render_widget(processes_widget, bottom[1]);
        })?;
        if event::poll(Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(200));
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}