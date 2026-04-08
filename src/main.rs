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
    widgets::{Block, Borders, Paragraph},
};

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

fn bytes_to_gb(bytes: u64) -> f64{
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn main() -> io::Result<()> {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    );

    let mut disks = Disks::new_with_refreshed_list();

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

        let mut cpu_text = format!(
            "Модель: {}\nОбщая загрузка: {:.1}%\n\nПо ядрам:\n",
            cpu_name, cpu_usage
        );

        for (i, cpu) in cpus.iter().enumerate() {
            cpu_text.push_str(&format!("CPU {}: {:.1}%\n", i, cpu.cpu_usage()));
        }

        let ram_text = format!(
            "Использование: {:.1}%\nTotal: {:.2} GB\nUsed: {:.2} GB\nFree: {:.2} GB\nAvailable: {:.2} GB",
            ram_percent,
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

            disks_text.push_str(&format!(
                "{} {:.1}% ({:.2} / {:.2} GB)\n",
                name, disk_percent, used_space_gb, total_space_gb
            ));
        }

        terminal.draw(|frame| {
            let area = frame.area();

            let cpu_height = 6 + cpus.len() as u16;
            let middle_height = 8_u16.max(3 + disks.list().len() as u16);

            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(cpu_height),
                    Constraint::Length(middle_height),
                    Constraint::Min(0),
                ])
                .split(area);

            let middle = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(vertical[1]);

            let cpu_widget = Paragraph::new(cpu_text.clone())
                .block(Block::default().title(" CPU ").borders(Borders::ALL));

            let ram_widget = Paragraph::new(ram_text.clone())
                .block(Block::default().title(" RAM").borders(Borders::ALL));

            let disks_widget = Paragraph::new(disks_text.clone())
                .block(Block::default().title(" DISKS").borders(Borders::ALL));

            frame.render_widget(cpu_widget, vertical[0]);
            frame.render_widget(ram_widget, middle[0]);
            frame.render_widget(disks_widget, middle[1]);
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