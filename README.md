# Carrot App

Carrot-App is a CosmWasm smart contract/module that demonstrates the use of the abstract stack. It allows users to autocompound rewards from providing to supercharged liquidity pools on Osmosis. The smart contract enables users to create a position in the liquidity pool, automatically withdraw rewards, and compound them.

## Features
Create a position in the liquidity pool
Deposit funds into the pool
Withdraw a specified amount or all funds
Autocompound rewards
## Entrypoints
### Execute Messages
CreatePosition: Creates a position in the liquidity pool
Deposit: Deposits funds into the pool
Withdraw: Withdraws a specified amount of funds from the pool
WithdrawAll: Withdraws all funds from the pool
Autocompound: Autocompounds rewards
### Query Messages
Balance: Returns the current balance in the pool
AvailableRewards: Returns the available rewards to be claimed
Config: Returns the current configuration of the contract
Position: Returns information about the user's position in the pool
CompoundStatus: Returns the current autocompound status (cooldown or ready)
## Bot
The repository also includes a bot that interacts with the Carrot-App contract. The bot fetches contract instances, checks permissions, and autocompounds rewards.
