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
    widgets::{Block, Borders, Paragraph, Cell, Row, Table, Sparkline},
};

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, Networks};

use nvml_wrapper::{
    enum_wrappers::device::TemperatureSensor,
    Nvml,
};

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

struct App{
    cpu_history: Vec<u64>,
    gpu_history: Vec<u64>,
    max_points: usize,
}

impl App {
    fn new(max_points: usize) -> Self {
        Self {
            cpu_history: Vec::new(),
            gpu_history: Vec::new(),
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

        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let free_memory = system.free_memory();
        let available_memory = system.available_memory();

        let total_memory_gb = bytes_to_gb(total_memory);
        let used_memory_gb = bytes_to_gb(used_memory);
        let free_memory_gb = bytes_to_gb(free_memory);
        let available_memory_gb = bytes_to_gb(available_memory);

        let ram_percent = (used_memory as f64 / total_memory as f64) * 100.0;
        let ram_bar = draw_bar(ram_percent, 20);

        let cpu_bar = draw_bar(cpu_usage as f64, 30);
        let mut cpu_text = format!(
            "CPU {} {:>5.1}%\n\n",
            cpu_bar, cpu_usage
        );

        let rows = 3;

        // сколько будет колонок
        let cols = (cpus.len() + rows - 1) / rows;

        for r in 0..rows {
            let mut line = String::new();

            for c in 0..cols {
                let index = c * rows + r;

                if index < cpus.len() {
                    let cpu = &cpus[index];

                    let core_text = format!(
                        "C{:<1} {:>5.1}% --°C",
                        index,
                        cpu.cpu_usage()
                    );

                    line.push_str(&core_text);

                    if c + 1 < cols {
                        line.push_str(" | "); // отступ между колонками
                    }
                }
            }

            cpu_text.push_str(&line);
            cpu_text.push('\n');
        }

        let mut gpu_usage_value = 0.0;
        let mut gpu_name = "No compatible device found".to_string();
        let mut gpu_memory_text = "Память GPU: Недоступно".to_string();
        let mut gpu_temp_text = "Температура: Недоступно".to_string();

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
                    gpu_memory_text = format!("Память GPU: {:.1} / {:.1} MB", used_mb, total_mb);
                }

                if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                    gpu_temp_text = format!("Температура: {}°C", temp);
                }
            }
        }

        app.push_cpu(cpu_usage as f64);
        app.push_gpu(gpu_usage_value);

        let gpu_bar = draw_bar(gpu_usage_value, 20);

        let gpu_text = if gpu_name == "No compatible device found" {
            "NO COMPATIBLE DEVICE FOUND".to_string()
        } else {
            format!(
                "Модель: {}\nЗагрузка GPU: {:>5.1}% [{}]\n{}\n{}",
                gpu_name, gpu_usage_value, gpu_bar, gpu_memory_text, gpu_temp_text
            )
        };

        let ram_text = format!(
            "Использование: {:.1}%\n[{}]\n\nTotal:     {:.2} GB\nUsed:      {:.2} GB\nFree:      {:.2} GB\nAvailable: {:.2} GB",
            ram_percent,
            ram_bar,
            total_memory_gb,
            used_memory_gb,
            free_memory_gb,
            available_memory_gb
        );

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
                "{}: {:.1}% {} \n({:.2} / {:.2} GB)\n\n",
                clean_name,
                disk_percent,
                disk_bar,
                used_space_gb,
                total_space_gb
            ));
        }

        let mut network_text = String::new();
        for (interface_name, network) in &networks {
            let rx_mb = bytes_to_mb(network.received());
            let tx_mb = bytes_to_mb(network.transmitted());
            let total_rx_mb = bytes_to_mb(network.total_received());
            let total_tx_mb = bytes_to_mb(network.total_transmitted());

            network_text.push_str(&format!(
                "{}\nRX: {:.2} MB | TX: {:.2} MB\nTotal RX: {:.2} MB\nTotal TX: {:.2} MB\n\n",
                interface_name,
                rx_mb,
                tx_mb,
                total_rx_mb,
                total_tx_mb
            ));
        }

        if network_text.is_empty() {
            network_text = "Нет доступных сетевых интерфейсов".to_string();
        }

        let mut processes: Vec<_> = system.processes().iter().collect();
        processes.sort_by(|a, b| b.1.memory().cmp(&a.1.memory()));

        let process_rows: Vec<Row> = processes
            .into_iter()
            .take(20)
            .map(|(pid, process)| {
                let cpu = format!("{:.1}", process.cpu_usage());
                let memory_mb = format!("{:.1}", process.memory() as f64 / 1024.0 / 1024.0);
                let name = truncate_text(&process.name().to_string_lossy(), 26);

                Row::new(vec![
                    Cell::from(pid.to_string()),
                    Cell::from(cpu),
                    Cell::from(memory_mb),
                    Cell::from(name),
                ])
            })
            .collect();

        terminal.draw(|frame| {
            let area = frame.area();

            let cpu_height = 9;
            let gpu_height = 7;

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(cpu_height),
                    Constraint::Length(gpu_height),
                    Constraint::Min(10),
                ])
                .split(area);

            let cpu_block = Block::default().title(" CPU ").borders(Borders::ALL);
            let gpu_block = Block::default().title(" GPU ").borders(Borders::ALL);

            let cpu_inner = cpu_block.inner(vertical[0]);
            let gpu_inner = gpu_block.inner(vertical[1]);

            frame.render_widget(cpu_block, vertical[0]);
            frame.render_widget(gpu_block, vertical[1]);

            let cpu_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Length(2),
                    Constraint::Percentage(50),
                ])
                .split(cpu_inner);

            let gpu_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(52),
                    Constraint::Length(10),
                    Constraint::Percentage(48),
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
                    Constraint::Length(8),
                    Constraint::Min(6),
                ])
                .split(bottom[0]);

            let left_top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(left_column[0]);

            let cpu_wave_lines = build_cpu_wave(&app.cpu_history, 7, cpu_split[0].width as usize);

            let cpu_wave_text = cpu_wave_lines.join("\n");

            let cpu_chart = Paragraph::new(cpu_wave_text);

            let gpu_chart = Sparkline::default()
                .data(&app.gpu_history)
                .max(100);

            let cpu_info_block = Block::default()
                .title(format!(" {} ", cpu_name))
                .borders(Borders::ALL);

            let cpu_info_inner = cpu_info_block.inner(cpu_split[2]);

            let cpu_info = Paragraph::new(cpu_text);

            let gpu_info = Paragraph::new(gpu_text);

            let ram_widget = Paragraph::new(ram_text)
                .block(Block::default().title(" RAM ").borders(Borders::ALL));

            let disks_widget = Paragraph::new(disks_text)
                .block(Block::default().title(" DISKS ").borders(Borders::ALL));

            let network_widget = Paragraph::new(network_text)
                .block(Block::default().title(" NETWORK ").borders(Borders::ALL));

            let processes_widget = Table::new(
                process_rows,
                [
                    Constraint::Length(9),
                    Constraint::Length(8),
                    Constraint::Length(11),
                    Constraint::Min(16),
                ],
            )
                .header(Row::new(vec!["PID", "CPU%", "RAM(MB)", "NAME"]))
                .column_spacing(3)
                .block(
                    Block::default()
                        .title(" PROCESSES (top by RAM) ")
                        .borders(Borders::ALL),
                );

            frame.render_widget(cpu_chart, cpu_split[0]);

            frame.render_widget(cpu_info_block, cpu_split[2]);
            frame.render_widget(cpu_info, cpu_info_inner);

            frame.render_widget(gpu_chart, gpu_split[0]);
            frame.render_widget(gpu_info, gpu_split[2]);


            frame.render_widget(ram_widget, left_top[0]);
            frame.render_widget(disks_widget, left_top[1]);
            frame.render_widget(network_widget, left_column[1]);

            frame.render_widget(processes_widget, bottom[1]);
        })?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}