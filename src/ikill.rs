use anyhow::Result;
use colored::Colorize;
use heim::process::processes;
use notify_rust::Notification;
use skim::prelude::*;
use smol::stream::StreamExt;
use std::io::Cursor;
use tabular::{Row, Table};

pub async fn run() -> Result<()> {
    let mut skim_args = Vec::new();
    let default_height = String::from("50%");
    let default_margin = String::from("0%");
    let default_layout = String::from("default");
    let default_theme = String::from(
        "matched:108,matched_bg:0,current:254,current_bg:236,current_match:151,current_match_bg:\
         236,spinner:148,info:144,prompt:110,cursor:161,selected:168,header:109,border:59",
    );

    skim_args.extend(
        std::env::var("SKIM_DEFAULT_OPTIONS")
            .ok()
            .and_then(|val| shlex::split(&val))
            .unwrap_or_default(),
    );

    let options = SkimOptionsBuilder::default()
        .margin(Some(
            skim_args
                .iter()
                .find(|arg| arg.contains("--margin") && *arg != &"--margin".to_string())
                .unwrap_or_else(|| {
                    skim_args
                        .iter()
                        .position(|arg| arg.contains("--margin"))
                        .map_or(&default_margin, |pos| &skim_args[pos + 1])
                }),
        ))
        .height(Some(
            skim_args
                .iter()
                .find(|arg| arg.contains("--height") && *arg != &"--height".to_string())
                .unwrap_or_else(|| {
                    skim_args
                        .iter()
                        .position(|arg| arg.contains("--height"))
                        .map_or(&default_height, |pos| &skim_args[pos + 1])
                }),
        ))
        .layout(
            skim_args
                .iter()
                .find(|arg| arg.contains("--layout") && *arg != &"--layout".to_string())
                .unwrap_or_else(|| {
                    skim_args
                        .iter()
                        .position(|arg| arg.contains("--layout"))
                        .map_or(&default_layout, |pos| &skim_args[pos + 1])
                }),
        )
        .color(Some(
            skim_args
                .iter()
                .find(|arg| {
                    arg.contains("--color") && *arg != &"--color".to_string() && !arg.contains("{}")
                })
                .unwrap_or_else(|| {
                    skim_args
                        .iter()
                        .position(|arg| arg.contains("--color"))
                        .map_or(&default_theme, |pos| &skim_args[pos + 1])
                }),
        ))
        .bind(
            skim_args
                .iter()
                .filter(|arg| arg.contains("--bind"))
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
        .reverse(skim_args.iter().any(|arg| arg.contains("--reverse")))
        .tac(skim_args.iter().any(|arg| arg.contains("--tac")))
        .nosort(skim_args.iter().any(|arg| arg.contains("--no-sort")))
        .inline_info(skim_args.iter().any(|arg| arg.contains("--inline-info")))
        .reverse(true)
        .multi(true)
        .build()
        .unwrap();

    let all_processes = match processes().await {
        Ok(processes) => processes.filter_map(|process| process.ok()).collect().await,
        Err(_) => Vec::with_capacity(0),
    };

    let mut table = Table::new("{:>}  {:>}");
    all_processes.iter().for_each(|ps| {
        smol::block_on(async {
            if let Ok(name) = ps.name().await {
                table.add_row(
                    Row::new()
                        .with_cell(name.red())
                        .with_cell(ps.pid().to_string().green()),
                );
            }
        });
    });

    // .with_cell(if let Ok(exe) = ps.exe().await {
    //     exe.into_os_string().into_string().unwrap_or("N/A".to_string()).blue()
    // } else {
    //     "N/A".to_string().red().bold()
    // }),

    let item_reader_opts = SkimItemReaderOption::default().ansi(true).build();
    let item_reader = SkimItemReader::new(item_reader_opts);
    let items = item_reader.of_bufread(Cursor::new(table.to_string()));

    let selected_items = Skim::run_with(&options, Some(items))
        .filter(|out| !out.is_abort)
        .map(|out| out.selected_items)
        .unwrap_or_else(Vec::new);

    let selected_pids = selected_items
        .iter()
        .map(|item| {
            item.text()
                .split_whitespace()
                .skip(1)
                .fold(String::with_capacity(0), |_, curr| curr.into())
        })
        .collect::<Vec<String>>();

    for process in all_processes {
        let selected_process = selected_pids.contains(&process.pid().to_string());

        if selected_process {
            match process.terminate().await {
                Ok(_) => {},
                Err(error) => {
                    eprintln!("{}: {}", "error".bold().red(), error.to_string());
                },
            }
        }
    }

    if !selected_pids.is_empty() {
        let avail = selected_items
            .iter()
            .map(|item| {
                item.text()
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect::<Vec<String>>()[0]
                    .clone()
            })
            .collect::<Vec<String>>()
            .iter()
            .fold(String::new(), |mut acc, k| {
                acc.push_str(&format!("{}, ", k));
                acc
            });

        let mut n = Notification::new();
        n.appname("ikill")
            .summary("Killed processes")
            .body(
                &avail
                    .strip_suffix(", ")
                    .map_or(avail.clone(), ToString::to_string),
            )
            .auto_icon()
            .icon("lock")
            .timeout(3000);

        n.show().unwrap();
    }

    Ok(())
}
