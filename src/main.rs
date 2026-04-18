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
    let filled = ((percent / 100.0) * width as f64) as usize;
    let empty = width.saturating_sub(filled);

    format!("{}{}", "█".repeat(filled), " ".repeat(empty))
}

fn main() -> io::Result<()> {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    );

    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();

    let nvml = Nvml::init().ok();

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
            ProcessRefreshKind::everything()
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

        let bar = draw_bar(cpu_usage as f64, 30);

        let mut cpu_text = format!(
            "Модель: {}\nОбщая загрузка: {:.1}%\n[{}]\n\nПо ядрам:\n",
            cpu_name, cpu_usage, bar
        );

        let mut i = 0;
        while i < cpus.len() {
            let left = &cpus[i];
            let left_text = format!("CPU {}: {:>5.1}%", i, left.cpu_usage());

            let right_text = if i + 1 < cpus.len() {
                let right = &cpus[i + 1];
                format!("CPU {}: {:>5.1}%", i + 1, right.cpu_usage())
            } else {
                String::new()
            };

            cpu_text.push_str(&format!("{:<20} {}\n", left_text, right_text));

            i += 2;
        }

        let gpu_text = if let Some(nvml) = &nvml {
            match nvml.device_by_index(0) {
                Ok(device) => {
                    let name = device
                        .name()
                        .unwrap_or_else(|_| "Unknown GPU".to_string());

                    let utilization = device.utilization_rates().ok();
                    let memory = device.memory_info().ok();
                    let temperature = device.temperature(TemperatureSensor::Gpu).ok();

                    let gpu_percent = utilization.map(|u| u.gpu as f64).unwrap_or(0.0);
                    let gpu_bar = draw_bar(gpu_percent, 30);

                    let memory_text = if let Some(mem) = memory {
                        let used_mb = bytes_to_mb(mem.used);
                        let total_mb = bytes_to_mb(mem.total);
                        format!("Память GPU: {:.1} / {:.1} MB", used_mb, total_mb)
                    } else {
                        "Память GPU: Недоступно".to_string()
                    };

                    let temperature_text = if let Some(temp) = temperature {
                        format!("Температура: {}°C", temp)
                    } else {
                        "Температура: Недоступно".to_string()
                    };

                    format!(
                        "Модель: {}\nЗагрузка GPU: {:.1}%\n[{}]\n\n{}\n{}",
                        name, gpu_percent, gpu_bar, memory_text, temperature_text
                    )
                }
                Err(_) => {
                    "NO COMPATIBLE DEVICE FOUND".to_string()
                }
            }
        } else {
            "NO COMPATIBLE DEVICE FOUND".to_string()
        };


        let ram_bar = draw_bar(ram_percent, 30);

        let ram_text = format!(
            "Использование: {:.1}%\n[{}]\n\nTotal:     {:.2} GB   Used:      {:.2} GB\nFree:      {:.2} GB    Available: {:.2} GB",
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
            let disk_bar = draw_bar(disk_percent, 20);

            disks_text.push_str(&format!(
                "{}: {:.1}%\n[{}]    ({:.2} / {:.2} GB)\n\n",
                clean_name,
                disk_percent,
                disk_bar,
                used_space_gb,
                total_space_gb
            ));
        }

        let mut network_text = String::new();
        for(interface_name, network) in &networks{
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
            .take(10)
            .map(|(pid, process)| {
                let cpu = format!("{:.1}", process.cpu_usage());
                let memory_mb = format!("{:.1}", process.memory() as f64 / 1024.0 / 1024.0);
                let name = truncate_text(&process.name().to_string_lossy(), 24);

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

            let cpu_height = 6 + cpus.len() as u16;
            let gpu_height = 8;
            let middle_height = 8_u16.max(3 + disks.list().len() as u16);
            let bottom_height = 14;

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(cpu_height),
                    Constraint::Length(gpu_height),
                    Constraint::Length(middle_height),
                    Constraint::Length(bottom_height),
                    Constraint::Min(0),
                ])
                .split(area);

            let middle = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(vertical[2]);

            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(35),
                    Constraint::Percentage(65),
                ])
                .split(vertical[3]);

            let cpu_widget = Paragraph::new(cpu_text.clone())
                .block(Block::default().title(" CPU ").borders(Borders::ALL));

            let gpu_widget = Paragraph::new(gpu_text.clone())
                .block(Block::default().title(" GPU ").borders(Borders::ALL));

            let ram_widget = Paragraph::new(ram_text.clone())
                .block(Block::default().title(" RAM").borders(Borders::ALL));

            let disks_widget = Paragraph::new(disks_text.clone())
                .block(Block::default().title(" DISKS").borders(Borders::ALL));

            let network_widget = Paragraph::new(network_text.clone())
                .block(Block::default().title(" NETWORK ").borders(Borders::ALL));

            let processes_widget = Table::new(
                process_rows,
                [
                    Constraint::Length(8),
                    Constraint::Length(8),
                    Constraint::Length(10),
                    Constraint::Min(10),
                ],
            )
                .header(
                    Row::new(vec!["PID", "CPU%", "RAM(MB)", "NAME"])
                )
                .column_spacing(1)
                .block(
                    Block::default()
                        .title(" PROCESSES (top 10 by RAM) ")
                        .borders(Borders::ALL),
                );


            frame.render_widget(cpu_widget, vertical[0]);
            frame.render_widget(gpu_widget, vertical[1]);
            frame.render_widget(ram_widget, middle[0]);
            frame.render_widget(disks_widget, middle[1]);
            frame.render_widget(network_widget, bottom[0]);
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