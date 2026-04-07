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

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System, ProcessRefreshKind, ProcessesToUpdate};

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}
fn main() -> io::Result<()> {
    let mut stdout = io::stdout();
    //Создаём один System и обновляем его дальше
    let mut system = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()).with_memory(MemoryRefreshKind::everything()),
    );

    let mut disks = Disks::new_with_refreshed_list();

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
        disks.refresh(true);
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );

        let cpu_usage = system.global_cpu_usage();
        let cpu_name = system.cpus()[0].brand();
        let cpus = system.cpus();


        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let free_memory = system.free_memory();
        let available_memory = system.available_memory();

        let total_memory_gb = bytes_to_gb(total_memory);
        let used_memory_gb = bytes_to_gb(used_memory);
        let free_memory_gb = bytes_to_gb(free_memory);
        let available_memory_gb = bytes_to_gb(available_memory);

        let ram_percent = (system.used_memory() as f64 / system.total_memory() as f64) * 100.0;

        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;

        writeln!(stdout, "=== SYSTEM MONITOR ===")?;
        writeln!(stdout, "CPU: {}", cpu_name)?;
        writeln!(stdout, "Общая загрузка: {:.1}%", cpu_usage)?;
        writeln!(stdout)?;

        writeln!(stdout, "По ядрам:")?;
        for (i, cpu) in cpus.iter().enumerate() {
            writeln!(stdout, "CPU {}: {:.1}%", i, cpu.cpu_usage())?;
        }

        writeln!(stdout)?;
        writeln!(stdout, "RAM: {:.1}%", ram_percent)?;
        writeln!(stdout, "Total:     {:.2} GB", total_memory_gb)?;
        writeln!(stdout, "Used:      {:.2} GB", used_memory_gb)?;
        writeln!(stdout, "Free:      {:.2} GB", free_memory_gb)?;
        writeln!(stdout, "Available: {:.2} GB", available_memory_gb)?;
        writeln!(stdout)?;

        writeln!(stdout, "DISKS:")?;
        for disk in disks.list(){
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

            writeln!(stdout, "{} {:.1}% ({:.2} / {:.2} GB)", name, disk_percent, used_space_gb, total_space_gb)?;
        }
        writeln!(stdout)?;

        writeln!(stdout, "PROCESSES:")?;
        writeln!(stdout, "PID      CPU%      RAM(MB)      NAME")?;

        let mut processes: Vec<_> = system.processes().iter().collect();
        processes.sort_by(|a, b|{
            let mem_a = a.1.memory();
            let mem_b = b.1.memory();
            mem_b.cmp(&mem_a)
        });

        for (pid, process) in processes.into_iter().take(20) {
            let cpu = process.cpu_usage();
            let memory_mb = process.memory() as f64 / 1024.0 / 1024.0;
            let name = process.name().to_string_lossy();

            writeln!(
                stdout,
                "{:<8} {:<9.1} {:<12.1} {}",
                pid,
                cpu,
                memory_mb,
                name
            )?;
        }
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