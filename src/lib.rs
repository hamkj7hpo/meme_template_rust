//! Meme Template v4.0 — Dynamic Vanity Program ID + SPMP + ZK Vault + Rotator
//! MOTHERSHIP: JBjKCmvSK3dMPfKk1WGD8nZfw8yAZHtuZ3GLo7NpCHX7
//! NO SPL, NO ANCHOR — Pure WASM + warp_core

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    hash::Hash,
};
use wasm_bindgen::prelude::*;
use std::str::FromStr;
use blstrs::{G1Projective};
use getrandom::getrandom::fill;
use borsh::{BorshSerialize, BorshDeserialize};

// ─────────────────────────────────────────────────────────────────────────────
// PROGRAM IDS
// ─────────────────────────────────────────────────────────────────────────────
pub const MOTHERSHIP_PROGRAM_ID: Pubkey = pubkey!("JBjKCmvSK3dMPfKk1WGD8nZfw8yAZHtuZ3GLo7NpCHX7");
pub const SPMP_SUFFIX: &str = "SPMP";

// ─────────────────────────────────────────────────────────────────────────────
// STATE
// ─────────────────────────────────────────────────────────────────────────────
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct MemeVault {
    pub mothership_pda: Pubkey,
    pub vanity_program_id: Pubkey,
    pub vanity_bump: u8,
    pub rotator_sk: [u8; 32],
    pub rotator_pk: Pubkey,
    pub last_rotation: i64,
    pub is_handshaken: bool,
    pub spmp_mint: String,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct HandshakeEvent {
    pub mothership: Pubkey,
    pub meme_program_id: Pubkey,
    pub deployer: Pubkey,
    pub spmp_mint: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// WASM CLIENT
// ─────────────────────────────────────────────────────────────────────────────
#[wasm_bindgen]
pub struct MemeTemplateClient {
    vault: Option<MemeVault>,
    bls_sk: [u8; 32],
    bls_pk: [u8; 48],
    deployer_kp: Keypair,
}

#[wasm_bindgen]
impl MemeTemplateClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut bls_sk = [0u8; 32];
        fill(&mut bls_sk).unwrap();
        let bls_pk = (G1Projective::generator() * blstrs::Scalar::from_bytes(&bls_sk).unwrap()).to_compressed();
        let deployer_kp = Keypair::new();

        MemeTemplateClient {
            vault: None,
            bls_sk,
            bls_pk,
            deployer_kp,
        }
    }

    // ───── CREATE TOKEN + HANDSHAKE → VANITY PROGRAM ID ─────
    #[wasm_bindgen]
    pub fn create_token_and_handshake(&mut self, name: &str, symbol: &str) -> JsValue {
        let deployer = self.deployer_kp.pubkey();

        // 1. Generate SPMP mint string
        let spmp_mint = format!("{}{}", symbol.to_uppercase(), SPMP_SUFFIX);

        // 2. Derive MOTHERSHIP PDA for this deployer
        let (mothership_pda, _bump) = Pubkey::find_program_address(
            &[b"contract", deployer.as_ref()],
            &MOTHERSHIP_PROGRAM_ID,
        );

        // 3. Generate vanity program ID with SPMP suffix
        let mut vanity_id: Pubkey;
        let mut bump: u8 = 0;
        let mut nonce: u8 = 0;

        loop {
            let seed = [
                b"meme",
                name.as_bytes(),
                spmp_mint.as_bytes(),
                &[nonce],
            ];
            let (pda, b) = Pubkey::find_program_address(&seed, &MOTHERSHIP_PROGRAM_ID);
            let pda_str = pda.to_string();
            if pda_str.ends_with(SPMP_SUFFIX) {
                vanity_id = pda;
                bump = b;
                break;
            }
            nonce += 1;
            if nonce == 0 { // overflow protection
                break;
            }
        }

        // 4. Generate rotator keypair (hourly rotation)
        let mut rotator_sk = [0u8; 32];
        fill(&mut rotator_sk).unwrap();
        let rotator_kp = Keypair::from_bytes(&[&rotator_sk, &[0; 32]].concat()).unwrap();
        let rotator_pk = rotator_kp.pubkey();

        // 5. Save vault state
        self.vault = Some(MemeVault {
            mothership_pda,
            vanity_program_id: vanity_id,
            vanity_bump: bump,
            rotator_sk,
            rotator_pk,
            last_rotation: js_sys::Date::now() as i64 / 1000,
            is_handshaken: true,
            spmp_mint: spmp_mint.clone(),
        });

        // 6. Serialize handshake event
        let event = HandshakeEvent {
            mothership: MOTHERSHIP_PROGRAM_ID,
            meme_program_id: vanity_id,
            deployer,
            spmp_mint,
        };

        let event_bytes = event.try_to_vec().unwrap();
        JsValue::from_serde(&serde_json::json!({
            "success": true,
            "vanity_program_id": vanity_id.to_string(),
            "spmp_mint": spmp_mint,
            "mothership_pda": mothership_pda.to_string(),
            "rotator_pk": rotator_pk.to_string(),
            "event": base64::encode(event_bytes),
        })).unwrap()
    }

    // ───── GETTERS ─────
    #[wasm_bindgen]
    pub fn get_vanity_id(&self) -> Option<String> {
        self.vault.as_ref().map(|v| v.vanity_program_id.to_string())
    }

    #[wasm_bindgen]
    pub fn get_spmp_mint(&self) -> Option<String> {
        self.vault.as_ref().map(|v| v.spmp_mint.clone())
    }

    #[wasm_bindgen]
    pub fn get_rotator_pk(&self) -> Option<String> {
        self.vault.as_ref().map(|v| v.rotator_pk.to_string())
    }

    #[wasm_bindgen]
    pub fn is_handshaken(&self) -> bool {
        self.vault.as_ref().map(|v| v.is_handshaken).unwrap_or(false)
    }

    // ───── ROTATE KEYS (HOURLY) ─────
    #[wasm_bindgen]
    pub fn rotate_keys(&mut self) -> bool {
        if let Some(vault) = &mut self.vault {
            let now = js_sys::Date::now() as i64 / 1000;
            if now - vault.last_rotation >= 3600 {
                let mut sk = [0u8; 32];
                fill(&mut sk).unwrap();
                let kp = Keypair::from_bytes(&[&sk, &[0; 32]].concat()).unwrap();
                vault.rotator_sk = sk;
                vault.rotator_pk = kp.pubkey();
                vault.last_rotation = now;
                return true;
            }
        }
        false
    }

    // ───── BLS SIGN (for swap) ─────
    #[wasm_bindgen]
    pub fn sign_swap(&self, amount_in: u64, is_buy: bool, min_out: u64, nonce: u64) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&amount_in.to_le_bytes());
        msg.push(if is_buy { 1 } else { 0 });
        msg.extend_from_slice(&min_out.to_le_bytes());
        msg.extend_from_slice(&nonce.to_le_bytes());
        if let Some(vault) = &self.vault {
            msg.extend_from_slice(vault.rotator_pk.as_ref());
        }
        blstrs::Scalar::hash_to_curve(&msg, b"SAFE-PUMP-SWAP", &[]).to_compressed().to_vec()
    }
}
