use sbor::Describe;
use scrypto::prelude::*;
/*
 * Non-Fungible badge that represents a player of a game with {Odd, Even} characteristic
 */
#[derive(ScryptoSbor, NonFungibleData)]
struct PlayerNFT {
    name: String,
    odd_or_even: OddOrEven,
}

#[blueprint]
mod oddoreven_module {
    struct OOE {
        player_badges_vault: Vault,
        player_badge_address: ResourceAddress,
        xrd_vault: Vault,
        game: Game,
    }

    impl OOE {
        /*
         * Instantiates the Odd or Even game giving a specific bet amount
         */
        pub fn instantiate_ooe_game(bet: Decimal) -> ComponentAddress {
            let player_badges: Bucket = ResourceBuilder::new_integer_non_fungible::<PlayerNFT>()
                .metadata("name", "Odd Or Even Game")
                .metadata("symbol", "OOE")
                .mint_initial_supply([
                    (
                        IntegerNonFungibleLocalId::new(1u64),
                        PlayerNFT {
                            name: "Even Player badge".to_string(),
                            odd_or_even: OddOrEven::Even,
                        },
                    ),
                    (
                        IntegerNonFungibleLocalId::new(2u64),
                        PlayerNFT {
                            name: "Odd Player badge".to_string(),
                            odd_or_even: OddOrEven::Odd,
                        },
                    ),
                ]);

            let pba = player_badges.resource_address();

            Self {
                player_badges_vault: Vault::with_bucket(player_badges),
                player_badge_address: pba,
                xrd_vault: Vault::new(RADIX_TOKEN),
                game: Game::instantiate_game(None, bet),
            }
            .instantiate()
            .globalize()
        }

        /*
         * Player can Join the Odd or Even game giving the necessary wager
         */
        pub fn join_ooe_game(&mut self, mut wager: Bucket) -> (Bucket, Bucket) {
            // Confirms the correct State of the game
            assert_eq!(
                self.game.state,
                State::AcceptingPlayers,
                "Not Accepting Players!"
            );
            // Confirms if game is full or not
            if self.game.players_list.as_ref().is_some() {
                assert!(
                    self.game.players_list.as_ref().unwrap().len() <= 2,
                    "This game is full!"
                );
            }
            // Confirms if the given wager is above the minimum amount
            assert!(
                wager.amount() >= self.game.bet_amount,
                "Not enough XRD Wagered! Minimum is {}",
                self.game.bet_amount
            );

            // Creates parameters to create Player object
            let mut player: Player = Player::empty();
            let badge: Bucket = self.player_badges_vault.take(dec!(1));
            let nft_id: NonFungibleLocalId = badge.non_fungible::<PlayerNFT>().local_id().clone();
            self.xrd_vault.put(wager.take(self.game.bet_amount));

            let badge_ooe = badge.non_fungible::<PlayerNFT>().data().odd_or_even;
            player.odd_or_even = Some(badge_ooe);

            // Adds player to the game with corresponding PlayerNFT id
            if let Some(players) = self.game.players_list.as_mut() {
                players.insert(nft_id, player);
            } else {
                let mut plyers = HashMap::new();
                plyers.insert(nft_id, player);
                self.game.players_list = Some(plyers);
            }

            // Updates Games State
            self.game.update_state();

            // Returns XRD if the amount given was greater than the games amount
            return (badge, wager);
        }

        pub fn pick_number(&mut self, number: u128, proof: Proof) {
            // Confirms Proof amount is equal to 1
            assert_eq!(proof.amount(), dec!("1"), "Invalid badge amount provided");
            // Confirms the correct State of the game
            assert_eq!(
                self.game.state,
                State::PickNumber,
                "It's not the time to pick a number!"
            );
            //assert!(number <= 6 && number >= 1, "You can only guess between 1-6");

            // Validates proof with player badge address
            let validated_proof = proof
                .validate_proof(self.player_badge_address)
                .expect("Wrong badge provided");

            // Creates player object using the given proof
            let pbadge = validated_proof.non_fungible::<PlayerNFT>();
            let pbadge_id = pbadge.local_id();
            let mut player = self.game.players_list.as_mut().unwrap().get_mut(&pbadge_id);
            player.as_mut().unwrap().number = number;

            // Updates State of the game
            self.game.update_state();
            info!("You number {} was registered!", number)
        }

        /*
         * Allows winner to withdraw XRD
         */
        pub fn withdraw_xrd(&mut self, proof: Proof) -> (Bucket, String) {
            // Confirms Proof amount is equal to 1
            assert_eq!(proof.amount(), dec!("1"), "Invalid badge amount provided");
            // Confirms the correct State of the game
            assert_eq!(
                self.game.state,
                State::Payout,
                "It's not time to withdraw XRD!"
            );

            // Validates proof with player badge address
            let validated_proof = proof
                .validate_proof(self.player_badge_address)
                .expect("Wrong badge provided");

            let nft_id = validated_proof
                .non_fungible::<PlayerNFT>()
                .local_id()
                .clone();
            // Confirms NFT ID is equal to the game winner
            assert_eq!(
                nft_id, self.game.winner,
                "You can't withdraw XRD because you didn't win. Better luck next time :)"
            );

            // Transfers winnings to winner
            let winnings = self.xrd_vault.amount();
            let payout = self.xrd_vault.take(winnings);

            // Updates State of the game
            self.game.update_state();
            (
                payout,
                format!(
                    "You withdrew {} XRD. Congratulations!",
                    winnings.to_string()
                ),
            )
        }

        // Get total wagered amount
        pub fn get_total_wagered_amount(&self) {
            info!("Total Wagered amount is {}", self.xrd_vault.amount())
        }

        // Get wagered amount
        pub fn get_wager_amount(&self) {
            info!("Wager amount is {}", self.game.bet_amount.to_string())
        }
    }
}

#[derive(ScryptoSbor)]
struct Game {
    state: State,
    players_list: Option<HashMap<NonFungibleLocalId, Player>>,
    winner: NonFungibleLocalId,
    bet_amount: Decimal,
}
impl Game {
    pub fn instantiate_game(
        players: Option<HashMap<NonFungibleLocalId, Player>>,
        bet: Decimal,
    ) -> Self {
        Self {
            state: State::AcceptingPlayers,
            players_list: players,
            winner: NonFungibleLocalId::integer(0),
            bet_amount: bet as Decimal,
        }
    }

    /*
     * Confirms ir both players picked a number already
     */
    pub fn both_picked(&self) -> bool {
        if let Some(players) = &self.players_list {
            if players.len() == 2 {
                let picks: Vec<u128> = players
                    .values()
                    .map(|player| player.number)
                    .filter(|pick| pick > &0u128)
                    .collect();

                return picks.len() == 2;
            }
        }
        false
    }

    /*
     * Based on each players pick, gets the winner
     */
    pub fn get_game_winner(&mut self) {
        assert_eq!(
            self.state,
            State::WinnerSelection,
            "Not the time to choose a winner yet!"
        );
        let players = self.players_list.as_ref().unwrap();
        let mut sum = 0;

        for (id, player) in players {
            sum += player.number;
            if sum % 2 == 0 {
                self.winner = id.clone();
            }
        }
        self.update_state();
    }

    /*
     * Updates State of the game
     * Game States:
     *  AcceptingPlayers: Two players are not in the game yet
     *  PickNumber: Both players haven't made their pick yet
     *  WinnerSelection: Winner is being decided
     *  Payout: Process of reward distribution begins where winner can withdraw XRD
     */
    pub fn update_state(&mut self) {
        match self.state {
            State::AcceptingPlayers => {
                if self.players_list.as_ref().unwrap().len() == 2 {
                    self.state = State::PickNumber;
                }
            }
            State::PickNumber => {
                if self.both_picked() {
                    self.state = State::WinnerSelection;
                    self.get_game_winner();
                }
            }
            State::WinnerSelection => {
                if self.winner != NonFungibleLocalId::integer(0) {
                    self.state = State::Payout;
                }
            }
            State::Payout => {
                self.state = State::Ended;
            }
            _ => (),
        }
    }
}

#[derive(Debug, Copy, Clone, Encode, Decode, PartialEq, Eq, Describe)]
enum State {
    AcceptingPlayers = 0,
    PickNumber = 1,
    WinnerSelection = 2,
    Payout = 3,
    Ended = 4,
}

#[derive(Debug, Copy, Clone, Encode, Decode, PartialEq, Eq, Describe)]
enum OddOrEven {
    Odd,
    Even,
}

#[derive(Debug, Copy, Clone, Encode, Decode, PartialEq, Eq, Categorize, Describe)]
struct Player {
    pub number: u128,
    odd_or_even: Option<OddOrEven>,
}
impl Player {
    pub fn empty() -> Player {
        return Self {
            number: 0u128,
            odd_or_even: None,
        };
    }
}
