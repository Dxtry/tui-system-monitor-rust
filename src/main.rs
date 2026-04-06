use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

use crossterm::{
    cursor::MoveTo,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

fn main() -> io::Result<()> {
    let mut stdout = io::stdout();

    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    for i in 1..=20 {
        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;

        writeln!(stdout, "=== SYSTEM MONITOR ===")?;
        writeln!(stdout, "Тест обновления экрана")?;
        writeln!(stdout, "Итерация: {i}")?;
        writeln!(stdout, "Если всё работает правильно, число должно меняться на одном экране.")?;

        stdout.flush()?;
        thread::sleep(Duration::from_secs(1));
    }

    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode()?;

    Ok(())
}