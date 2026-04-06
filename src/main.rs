use std::{thread, time::Duration};
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
    cursor::MoveTo,
};
use std::io::{stdout};

fn main() {
    loop {
        // Очистка терминала
        execute!(
            stdout(),
            Clear(ClearType::All),
            MoveTo(0, 0)
        ).unwrap();

        println!("=== SYSTEM MONITOR ===");
        println!("Приложение работает...");
        println!("Обновление каждую секунду");

        thread::sleep(Duration::from_secs(1));
    }
}