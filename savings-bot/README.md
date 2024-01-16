# Savings bot

Savings bot allows to call autocompound method of all savings-app users

## Installation

To install savings-bot run this command:

```bash
cargo install --path savings-bot
```

## Automation

Cron daemon could be used for automation of this tool, example of crontab entry:

```bash
0 0 * * * $HOME/.cargo/bin/savings-bot
```

This will run savings bot daily at 00:00.
