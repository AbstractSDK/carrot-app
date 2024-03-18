# Carrot App

Carrot-App is a CosmWasm smart contract/module that demonstrates the use of the abstract stack.
The carrot-app is useful because it allows investors to maximise the yield they earn from their assets. It should allow investors to make different strategies like creating USDC/USDT positions or lend stable coins and earn yield from that. Of course investors can do this manually, hence bypassing the need for such carrot-app contract, however using this contract allows a recurrent autocompounding handled by an external bot and, more importantly, from a developer perspecitve, it abstracts away the need to think about what underlying dex or lending platform the investor will be using.
The current version V1 of the carrot-app allows users to autocompound rewards from providing to supercharged liquidity pools on Osmosis but more DEXes will be supported in the future. The smart contract enables users to create a position in the liquidity pool, automatically withdraw rewards, and compound them. In V2, the carrot-app will allow more yield strategies like lending.
This contract does not hold custody of the funds, instead it gets the authz permissions from a user or an abstract account to act on their behalf.

## Agents involved:

- Carrot app developer: develops, maintains this contract and publishes it to Abstract [repo](https://github.com/AbstractSDK/abstract/tree/main/modules/contracts/apps)
- Investor/user: Installs this app on their abstract account and deposit some funds that they want to get yield from.
- Bot: Autcompounds the position of all investors to earn incentives.
- Abstract developer: Develops the tooling that the carrot-app developer needs to make their life easier.

## Features

- Create a position in the liquidity pool
- Deposit funds into the pool
- Withdraw a specified amount or all funds
- Autocompound rewards

## Entrypoints

### Execute Messages

- CreatePosition: Creates a position in the liquidity pool
- Deposit: Deposits funds into the pool
- Withdraw: Withdraws a specified amount of funds from the pool
- WithdrawAll: Withdraws all funds from the pool
- Autocompound: Autocompounds rewards

### Query Messages

- Balance: Returns the current balance in the pool
- AvailableRewards: Returns the available rewards to be claimed
- Config: Returns the current configuration of the contract
- Position: Returns information about the user's position in the pool
- CompoundStatus: Returns the current autocompound status (cooldown or ready)

## Bot

The repository also includes a bot that interacts with the Carrot-App contract. The bot fetches contract instances, checks permissions, and autocompounds rewards.
