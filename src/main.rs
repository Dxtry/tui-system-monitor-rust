use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

use crossterm::{
    execute,
    cursor::MoveTo,
    terminal::{
        disable_raw_mode, enable_raw_mode,
        Clear, ClearType,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
fn main() -> io::Result<()> {
    let mut stdout = io::stdout();
    //Создаём один System и обновляем его дальше
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    );
    //Первый refresh нужен, чтобы CPU потом считался корректно
    system.refresh_cpu_all();
    system.refresh_memory();


    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    loop {
        thread::sleep(Duration::from_secs(1));

        //Второй и последующие refresh дают реальные значения
        system.refresh_cpu_all();
        system.refresh_memory();

        let cpu_usage = system.global_cpu_usage();
        let used_memory_gb = system.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let total_memory_gb = system.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
        let ram_precent = (system.used_memory() as f64 / system.total_memory() as f64) * 100.0;

        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;

        writeln!(stdout, "=== SYSTEM MONITOR ===")?;
        writeln!(stdout, "CPU: {:.1}%", cpu_usage)?;
        writeln!(stdout, "RAM: {:.1}% ({:.2} / {:.2} GB)", ram_precent, used_memory_gb, total_memory_gb)?;
        writeln!(stdout)?;
        writeln!(stdout, "Нажми Ctrl+C для выхода")?;

        stdout.flush()?;
    }

    #[allow(unreachable_code)]
    {
        execute!(stdout, LeaveAlternateScreen)?;
        disable_raw_mode()?;
        Ok(())
    }
}