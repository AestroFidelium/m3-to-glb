use clap::CommandFactory;
use clap_complete::{generate_to, shells::Zsh};
use std::env;
use std::fs;
use std::path::Path;

// Включаем код из cli.rs, чтобы иметь доступ к структуре Cli
include!("src/cli.rs");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Получаем имя приложения из переменной окружения Cargo
    let app_name = env::var("CARGO_PKG_NAME")?;
    let out_dir = "completions"; // Папка, куда сохраним результат

    // Создаем директорию, если её нет
    fs::create_dir_all(out_dir)?;

    let mut cmd = Cli::command();

    // Генерируем Zsh подсказки
    generate_to(
        Zsh, &mut cmd,  // Наша команда
        &app_name, // Имя из env
        out_dir,   // Куда сохранить (_m3-to-glb будет создан там)
    )?;

    println!("cargo:rerun-if-changed=src/cli.rs");
    Ok(())
}
