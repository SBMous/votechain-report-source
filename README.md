# Pnyx Report Source Code
> Undergraduate dissertation project codebase
> 
> Author: George Yarnley

## What is this?
This is the accompanying source code for the report 'Pnyx; A foundation for decentralised democracy without trusted parties', produced as part of my undergraduate degree. The report itself is present under 'FinalReport-Pnyx.pdf'

This version of the code includes some of the early ideas of the Pnyx ecosystem and represents a rudimentary implementation of the core concepts.

## Project Layout
The project is broken into three functional components:
1. 'lib' - Contains shared structures such as a generic signed container and ballots
2. 'node' - Contains all of the functionality for running a votechain node
3. 'client' - Contains user functionality for interacting with the node, currently just casting votes and generating test parameters

## Simulation
For simulations, a set of identities are provided in `./temp/identities` - These keypairs are automatically included into the census and so can have votes cast against them. This `./temp` folder also includes a trustee key which all votes are encrypted against.

The node has three important arguments
`--chain-postfix` - Adjusts the path which the blockchain for this node is stored under. If not provided, a random one is generated at startup
`--test-append` - Determines how many blocks should be appended to this chain on startup [default: 0]
`--test-identity` - Which of the available test identities should we use. Expects a number 1-20

The client has two primary commands: `cast` & `init-keys`

`cast` is used for casting votes. It should be able to automatically interface with a vote node on the local network running under identity '1'
Vote casting requires the following arguments:
`--issue` - An identifier representing the specific issue they wish to vote on
`--verdict` - The user's vote intent. If present, vote yes, if not, vote no.
`--id` - The identity the user wishes to sign as. Expects a number 1-20

The `init-keys` command is unlikely to be needed, as identities are pregenerated, but can be used to generate a new signing key pair which is written to a path provided in the config file