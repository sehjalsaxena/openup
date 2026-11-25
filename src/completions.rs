//! Shell completion scripts for OpenUp commands and options.

pub fn generate_completion(shell: &str) -> Result<String, String> {
    match shell.to_ascii_lowercase().as_str() {
        "powershell" | "pwsh" => Ok(powershell_completions()),
        "bash" => Ok(bash_completions()),
        "zsh" => Ok(zsh_completions()),
        "fish" => Ok(fish_completions()),
        other => Err(format!(
            "Unsupported shell '{other}'. Supported shells: powershell, bash, zsh, fish"
        )),
    }
}

fn powershell_completions() -> String {
    r#"# OpenUp tab completion for PowerShell
Register-ArgumentCompleter -Native -CommandName 'openup' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $elements = $commandAst.CommandElements
    $count = $elements.Count

    $subcommands = @(
        'now', 'setup', 'creds', 'status', 'doctor',
        'keeper', 'startup', 'portals', 'wifi', 'notify',
        'tray', 'log', 'update', 'completions', '--version', '--help'
    )

    if ($count -eq 1 -or ($count -eq 2 -and $wordToComplete)) {
        $subcommands | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
        }
        return
    }

    $first = $elements[1].Extent.Text
    switch ($first) {
        'keeper' {
            @('start', 'stop', 'run') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
        'startup' {
            @('on', 'off') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
        'portals' {
            @('trust', 'forget') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
        'wifi' {
            @('add', 'remove', 'list', 'clear') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
        'notify' {
            @('on', 'off', 'test') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
        'update' {
            @('--check', '--force') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
        'completions' {
            @('powershell', 'bash', 'zsh', 'fish') | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
            }
        }
    }
}
"#
    .to_string()
}

fn bash_completions() -> String {
    r#"# OpenUp tab completion for Bash
_openup() {
    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    local subcommands="now setup creds status doctor keeper startup portals wifi notify tray log update completions --version --help"

    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=( $(compgen -W "$subcommands" -- "$cur") )
        return 0
    fi

    case "${COMP_WORDS[1]}" in
        keeper)
            COMPREPLY=( $(compgen -W "start stop run --once --dry-run" -- "$cur") )
            ;;
        startup)
            COMPREPLY=( $(compgen -W "on off --now" -- "$cur") )
            ;;
        portals)
            COMPREPLY=( $(compgen -W "trust forget" -- "$cur") )
            ;;
        wifi)
            COMPREPLY=( $(compgen -W "add remove list clear" -- "$cur") )
            ;;
        notify)
            COMPREPLY=( $(compgen -W "on off test" -- "$cur") )
            ;;
        update)
            COMPREPLY=( $(compgen -W "--check --force" -- "$cur") )
            ;;
        completions)
            COMPREPLY=( $(compgen -W "bash zsh fish powershell" -- "$cur") )
            ;;
        log)
            COMPREPLY=( $(compgen -W "-n" -- "$cur") )
            ;;
    esac
}
complete -F _openup openup
"#
    .to_string()
}

fn zsh_completions() -> String {
    r#"#compdef openup
# OpenUp tab completion for Zsh

_openup() {
    local -a commands
    commands=(
        'now:Sign in now'
        'setup:Guided first-run setup'
        'creds:Save or replace username and password'
        'status:Show what OpenUp is doing'
        'doctor:Check everything and explain what is wrong'
        'keeper:Manage the background keeper'
        'startup:Configure run-on-login startup'
        'portals:Manage trusted captive portals'
        'wifi:Manage allowed campus Wi-Fi networks'
        'notify:Manage desktop notifications'
        'tray:Run in desktop notification tray'
        'log:Show recent log lines'
        'update:Update OpenUp to the latest version'
        'completions:Generate shell completion script'
        '--version:Show version information'
        '--help:Show help message'
    )

    if (( CURRENT == 2 )); then
        _describe -t commands 'openup command' commands
    else
        case "$words[2]" in
            keeper)
                _values 'keeper action' 'start[Start keeper in background]' 'stop[Stop keeper]' 'run[Run keeper in foreground]'
                ;;
            startup)
                _values 'startup action' 'on[Enable startup on login]' 'off[Disable startup on login]'
                ;;
            portals)
                _values 'portals action' 'trust[Trust a portal]' 'forget[Forget a trusted portal]'
                ;;
            wifi)
                _values 'wifi action' 'add[Add allowed SSID]' 'remove[Remove allowed SSID]' 'list[List allowed SSIDs]' 'clear[Clear allowed SSIDs]'
                ;;
            notify)
                _values 'notify action' 'on[Turn notifications on]' 'off[Turn notifications off]' 'test[Send test notification]'
                ;;
            update)
                _values 'update options' '--check[Only check for newer version]' '--force[Reinstall even if current]'
                ;;
            completions)
                _values 'shell' 'bash' 'zsh' 'fish' 'powershell'
                ;;
        esac
    fi
}

_openup "$@"
"#
    .to_string()
}

fn fish_completions() -> String {
    r#"# OpenUp tab completion for Fish
complete -c openup -f
complete -c openup -n "__fish_use_subcommand" -a now -d "Sign in now"
complete -c openup -n "__fish_use_subcommand" -a setup -d "Guided first-run setup"
complete -c openup -n "__fish_use_subcommand" -a creds -d "Save or replace credentials"
complete -c openup -n "__fish_use_subcommand" -a status -d "Show daemon status"
complete -c openup -n "__fish_use_subcommand" -a doctor -d "Diagnose issues"
complete -c openup -n "__fish_use_subcommand" -a keeper -d "Manage keeper daemon"
complete -c openup -n "__fish_use_subcommand" -a startup -d "Configure run-at-login"
complete -c openup -n "__fish_use_subcommand" -a portals -d "Manage trusted portals"
complete -c openup -n "__fish_use_subcommand" -a wifi -d "Manage allowed Wi-Fi SSIDs"
complete -c openup -n "__fish_use_subcommand" -a notify -d "Configure notifications"
complete -c openup -n "__fish_use_subcommand" -a tray -d "Run in system tray"
complete -c openup -n "__fish_use_subcommand" -a log -d "Show recent logs"
complete -c openup -n "__fish_use_subcommand" -a update -d "Update to latest release"
complete -c openup -n "__fish_use_subcommand" -a completions -d "Generate shell completions"

complete -c openup -n "__fish_seen_subcommand_from keeper" -a "start stop run"
complete -c openup -n "__fish_seen_subcommand_from startup" -a "on off"
complete -c openup -n "__fish_seen_subcommand_from portals" -a "trust forget"
complete -c openup -n "__fish_seen_subcommand_from wifi" -a "add remove list clear"
complete -c openup -n "__fish_seen_subcommand_from notify" -a "on off test"
complete -c openup -n "__fish_seen_subcommand_from update" -l check -d "Check for updates without installing"
complete -c openup -n "__fish_seen_subcommand_from update" -l force -d "Force re-download even if up to date"
complete -c openup -n "__fish_seen_subcommand_from completions" -a "bash zsh fish powershell"
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_shells_produce_valid_scripts() {
        for shell in ["powershell", "pwsh", "bash", "zsh", "fish"] {
            let script = generate_completion(shell).expect("should succeed");
            assert!(script.contains("openup"));
            assert!(script.contains("keeper"));
            assert!(script.contains("update"));
            assert!(script.contains("wifi"));
        }
    }

    #[test]
    fn invalid_shell_errors() {
        assert!(generate_completion("invalid_shell").is_err());
    }
}
