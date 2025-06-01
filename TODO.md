# TODO
- [ ] Devise appropriate message deduplication strategy
- [x] Setup Vote Serialisation Format
- [ ] Build client setup process
    - [ ] Hook into Kademlia node discovery insired by SafeNetwork
        - https://github.com/maidsafe/safe_network/blob/main/sn_networking/src/event.rs#L890
        - https://github.com/maidsafe/safe_network/blob/main/sn_client/src/api.rs#L244
- [ ] Setup ZK Proof for votes
- [x] Work out why votes aren't arriving at the target node