//! Instructor code hashing (Argon2id, PHC strings), group seeds (HMAC-SHA256, ch. 01 §2.8) and sealed group packs
//! (Argon2id key derivation + AES-256-GCM, header authenticated as associated data). Contract: docs/distribution.md §3.
//! Generic: holds no key, no code, no mission value.
//!
//! Sealed pack envelope (UTF-8 JSON, fields in this order):
//! `format` "n7rt-group-pack", `version` 1, `mission`, `cohort`, `group`, `pack_id` (32 lowercase hex),
//! `kdf` {`alg` "argon2id", `m_kib`, `t`, `p`, `salt` (base64, 16 bytes)}, `aead` {`alg` "aes-256-gcm", `nonce`
//! (base64, 12 bytes)}, `ciphertext` (base64, AES-256-GCM output with its 16-byte tag appended).
//!
//! Associated data: the compact JSON (no whitespace, serde_json escaping) of the envelope without `ciphertext`, keys
//! in exactly this order: `{"format":…,"version":…,"mission":…,"cohort":…,"group":…,"pack_id":…,
//! "kdf":{"alg":…,"m_kib":…,"t":…,"p":…,"salt":…},"aead":{"alg":…,"nonce":…}}`. The reader rebuilds it from the
//! parsed values, so any altered header field fails the tag check. Key = Argon2id v0x13 (normalized code, salt, m_kib,
//! t, p), 32 bytes; it lives only in memory.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    /// Not a sealed pack, unknown version or malformed field (French message, no secret inside).
    Format(String),
    /// Wrong code, or header/ciphertext altered (AES-GCM tag mismatch).
    WrongCode,
    Crypto(String),
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SealError::Format(m) => write!(f, "{m}"),
            SealError::WrongCode => write!(f, "code incorrect ou fichier altéré"),
            SealError::Crypto(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for SealError {}

/// Argon2id cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

/// Defaults: 64 MiB, 3 passes, 1 lane (about 0.3–0.5 s on a laptop).
pub const KDF_DEFAULT: KdfParams = KdfParams { m_kib: 65536, t: 3, p: 1 };

/// Accepted cost bounds when reading a pack or a PHC string (a forged file must not exhaust memory).
const MAX_M_KIB: u32 = 1 << 20;
const MAX_T: u32 = 16;
const MAX_P: u32 = 16;

const PACK_FORMAT: &str = "n7rt-group-pack";
const PACK_VERSION: u32 = 1;
const KDF_ALG: &str = "argon2id";
const AEAD_ALG: &str = "aes-256-gcm";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;

/// Crockford base32 alphabet (no I, L, O, U).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Canonical form of a typed code: uppercase, spaces and dashes removed.
pub fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '\u{2010}'..='\u{2015}' | '\u{2212}'))
        .flat_map(char::to_uppercase)
        .collect()
}

fn check_params(k: KdfParams) -> Result<Params, SealError> {
    let bad = || SealError::Format("paramètres de dérivation de clé hors limites".into());
    if k.t == 0 || k.t > MAX_T || k.p == 0 || k.p > MAX_P || k.m_kib > MAX_M_KIB || k.m_kib < 8 * k.p {
        return Err(bad());
    }
    Params::new(k.m_kib, k.t, k.p, Some(KEY_LEN)).map_err(|_| bad())
}

fn argon2id(k: KdfParams) -> Result<Argon2<'static>, SealError> {
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, check_params(k)?))
}

/// Argon2id PHC string of the normalized code, random salt.
pub fn hash_code(code: &str) -> Result<String, SealError> {
    let norm = normalize_code(code);
    if norm.is_empty() {
        return Err(SealError::Format("code vide".into()));
    }
    let salt = SaltString::encode_b64(&random_bytes(SALT_LEN))
        .map_err(|_| SealError::Crypto("sel de hachage invalide".into()))?;
    let hash = argon2id(KDF_DEFAULT)?
        .hash_password(norm.as_bytes(), &salt)
        .map_err(|_| SealError::Crypto("hachage du code impossible".into()))?;
    Ok(hash.to_string())
}

/// Constant-time verification of a typed code against a PHC string; false on any malformed input.
pub fn verify_code(phc: &str, code: &str) -> bool {
    let Ok(hash) = PasswordHash::new(phc) else { return false };
    if hash.algorithm.as_str() != KDF_ALG {
        return false;
    }
    let Ok(p) = Params::try_from(&hash) else { return false };
    let k = KdfParams { m_kib: p.m_cost(), t: p.t_cost(), p: p.p_cost() };
    if check_params(k).is_err() {
        return false;
    }
    // The verifier recomputes with the PHC's own parameters and compares the outputs in constant time.
    Argon2::default().verify_password(normalize_code(code).as_bytes(), &hash).is_ok()
}

/// New human-readable group code: 16 symbols of Crockford base32 in 4 groups, e.g. "7K4Q-M2ZD-9XRT-4WBC" (80 bits).
pub fn new_group_code() -> String {
    let bytes = random_bytes(10);
    let v = bytes.iter().fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
    let mut out = String::with_capacity(19);
    for i in 0..16 {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(CROCKFORD[((v >> (75 - 5 * i)) & 31) as usize] as char);
    }
    out
}

/// 32 random bytes from the operating system.
pub fn new_master_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    fill_random(&mut k);
    k
}

/// `n` random bytes from the operating system.
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    fill_random(&mut b);
    b
}

fn fill_random(buf: &mut [u8]) {
    getrandom::fill(buf).expect("générateur aléatoire du système indisponible");
}

/// Group seed of ch. 01 §2.8: HMAC-SHA256(master, lp(mission) ‖ lp(cohort) ‖ lp(group)), lp = u32 big-endian length
/// then UTF-8 bytes.
pub fn group_seed(master: &[u8; 32], mission: &str, cohort: &str, group: &str) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(master).expect("HMAC accepts any key length");
    for part in [mission, cohort, group] {
        mac.update(&(part.len() as u32).to_be_bytes());
        mac.update(part.as_bytes());
    }
    mac.finalize().into_bytes().into()
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Clear header of a sealed group pack (authenticated, not encrypted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackHeader {
    pub mission: String,
    pub cohort: String,
    pub group: String,
    /// 16 random bytes, lowercase hex.
    pub pack_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KdfField {
    alg: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AeadField {
    alg: String,
    nonce: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    mission: String,
    cohort: String,
    group: String,
    pack_id: String,
    kdf: KdfField,
    aead: AeadField,
    ciphertext: String,
}

/// The authenticated header: the envelope without `ciphertext`, same field order.
#[derive(Serialize)]
struct AadView<'a> {
    format: &'a str,
    version: u32,
    mission: &'a str,
    cohort: &'a str,
    group: &'a str,
    pack_id: &'a str,
    kdf: &'a KdfField,
    aead: &'a AeadField,
}

impl Envelope {
    fn aad(&self) -> Vec<u8> {
        serde_json::to_vec(&AadView {
            format: &self.format,
            version: self.version,
            mission: &self.mission,
            cohort: &self.cohort,
            group: &self.group,
            pack_id: &self.pack_id,
            kdf: &self.kdf,
            aead: &self.aead,
        })
        .expect("serializable header")
    }

    fn header(&self) -> PackHeader {
        PackHeader {
            mission: self.mission.clone(),
            cohort: self.cohort.clone(),
            group: self.group.clone(),
            pack_id: self.pack_id.clone(),
        }
    }
}

fn format_err(what: &str) -> SealError {
    SealError::Format(format!("pack de groupe : {what}"))
}

fn check_header(h: &PackHeader) -> Result<(), SealError> {
    for (name, v) in [("mission", &h.mission), ("cohort", &h.cohort), ("group", &h.group)] {
        if v.is_empty() || v.chars().count() > 64 || v.chars().any(char::is_control) {
            return Err(format_err(&format!("champ « {name} » invalide")));
        }
    }
    let id_ok = h.pack_id.len() == 32 && h.pack_id.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
    if !id_ok {
        return Err(format_err("champ « pack_id » invalide (32 chiffres hexadécimaux minuscules attendus)"));
    }
    Ok(())
}

fn parse_envelope(file: &[u8]) -> Result<Envelope, SealError> {
    let probe: serde_json::Value =
        serde_json::from_slice(file).map_err(|_| format_err("fichier illisible (JSON invalide)"))?;
    if probe.get("format").and_then(|v| v.as_str()) != Some(PACK_FORMAT) {
        return Err(format_err("ce fichier n'est pas un pack de groupe N7RT"));
    }
    match probe.get("version").and_then(|v| v.as_u64()) {
        Some(v) if v == u64::from(PACK_VERSION) => {}
        Some(v) => return Err(format_err(&format!("version {v} non prise en charge"))),
        None => return Err(format_err("champ « version » invalide")),
    }
    let env: Envelope =
        serde_json::from_value(probe).map_err(|_| format_err("champ manquant, en trop ou mal typé"))?;
    check_header(&env.header())?;
    Ok(env)
}

fn derive_key(code: &str, salt: &[u8], k: KdfParams) -> Result<[u8; KEY_LEN], SealError> {
    let mut key = [0u8; KEY_LEN];
    argon2id(k)?
        .hash_password_into(normalize_code(code).as_bytes(), salt, &mut key)
        .map_err(|_| SealError::Crypto("dérivation de clé impossible".into()))?;
    Ok(key)
}

/// Seals `plaintext` (the student pack JSON) with a key derived from `code`: the JSON envelope of
/// docs/distribution.md §3.1, UTF-8.
pub fn seal_pack(header: &PackHeader, plaintext: &[u8], code: &str) -> Result<Vec<u8>, SealError> {
    seal_pack_with(header, plaintext, code, KDF_DEFAULT)
}

/// [`seal_pack`] with explicit Argon2id costs (tests; production uses [`KDF_DEFAULT`]).
pub fn seal_pack_with(header: &PackHeader, plaintext: &[u8], code: &str, kdf: KdfParams) -> Result<Vec<u8>, SealError> {
    check_header(header)?;
    check_params(kdf)?;
    if normalize_code(code).is_empty() {
        return Err(SealError::Format("code vide".into()));
    }
    let salt = random_bytes(SALT_LEN);
    let nonce = random_bytes(NONCE_LEN);
    let mut env = Envelope {
        format: PACK_FORMAT.into(),
        version: PACK_VERSION,
        mission: header.mission.clone(),
        cohort: header.cohort.clone(),
        group: header.group.clone(),
        pack_id: header.pack_id.clone(),
        kdf: KdfField { alg: KDF_ALG.into(), m_kib: kdf.m_kib, t: kdf.t, p: kdf.p, salt: B64.encode(&salt) },
        aead: AeadField { alg: AEAD_ALG.into(), nonce: B64.encode(&nonce) },
        ciphertext: String::new(),
    };
    let key = derive_key(code, &salt, kdf)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SealError::Crypto("clé de chiffrement invalide".into()))?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: &env.aad() })
        .map_err(|_| SealError::Crypto("chiffrement impossible".into()))?;
    env.ciphertext = B64.encode(ct);
    let mut out = serde_json::to_vec_pretty(&env).map_err(|_| SealError::Crypto("sérialisation impossible".into()))?;
    out.push(b'\n');
    Ok(out)
}

/// Seals a local file of the application with a 32-byte key that lives only in memory (AES-256-GCM, random nonce
/// prepended, `aad` authenticated): files a progression keeps on disk without disclosing what the group pack holds.
pub fn seal_local(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
    let nonce = random_bytes(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| SealError::Crypto("clé de chiffrement invalide".into()))?;
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad })
        .map_err(|_| SealError::Crypto("chiffrement impossible".into()))?;
    Ok([nonce, ct].concat())
}

/// Opens a [`seal_local`] blob; Err when the key, the associated data or a byte differs.
pub fn open_local(key: &[u8; 32], aad: &[u8], blob: &[u8]) -> Result<Vec<u8>, SealError> {
    if blob.len() < NONCE_LEN + 16 {
        return Err(SealError::Crypto("fichier scellé tronqué".into()));
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| SealError::Crypto("clé de chiffrement invalide".into()))?;
    cipher.decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad }).map_err(|_| SealError::Crypto("fichier scellé illisible".into()))
}

/// Reads the clear header without the code.
pub fn read_pack_header(file: &[u8]) -> Result<PackHeader, SealError> {
    Ok(parse_envelope(file)?.header())
}

/// Opens a sealed pack: header and plaintext, or WrongCode.
pub fn open_pack(file: &[u8], code: &str) -> Result<(PackHeader, Vec<u8>), SealError> {
    let env = parse_envelope(file)?;
    if env.kdf.alg != KDF_ALG {
        return Err(format_err("algorithme de dérivation de clé non pris en charge"));
    }
    if env.aead.alg != AEAD_ALG {
        return Err(format_err("algorithme de chiffrement non pris en charge"));
    }
    let kdf = KdfParams { m_kib: env.kdf.m_kib, t: env.kdf.t, p: env.kdf.p };
    check_params(kdf)?;
    let salt = B64.decode(&env.kdf.salt).map_err(|_| format_err("champ « kdf.salt » invalide"))?;
    if !(8..=64).contains(&salt.len()) {
        return Err(format_err("champ « kdf.salt » invalide"));
    }
    let nonce = B64.decode(&env.aead.nonce).map_err(|_| format_err("champ « aead.nonce » invalide"))?;
    if nonce.len() != NONCE_LEN {
        return Err(format_err("champ « aead.nonce » invalide"));
    }
    let ct = B64.decode(&env.ciphertext).map_err(|_| format_err("champ « ciphertext » invalide"))?;
    if ct.len() < TAG_LEN {
        return Err(format_err("champ « ciphertext » tronqué"));
    }
    let key = derive_key(code, &salt, kdf)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SealError::Crypto("clé de chiffrement invalide".into()))?;
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: &env.aad() })
        .map_err(|_| SealError::WrongCode)?;
    Ok((env.header(), plain))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams { m_kib: 64, t: 1, p: 1 };
    const CODE: &str = "7K4Q-M2ZD-9XRT-4WBC";

    fn header() -> PackHeader {
        PackHeader {
            mission: "demo".into(),
            cohort: "2026-27".into(),
            group: "G01".into(),
            pack_id: "00112233445566778899aabbccddeeff".into(),
        }
    }

    fn json(file: &[u8]) -> serde_json::Value {
        serde_json::from_slice(file).unwrap()
    }

    fn bytes(v: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(v).unwrap()
    }

    #[test]
    fn round_trip_with_default_costs() {
        let plain = br#"{"pack":"student"}"#;
        let file = seal_pack(&header(), plain, CODE).unwrap();
        let v = json(&file);
        assert_eq!(v["format"], "n7rt-group-pack");
        assert_eq!(v["version"], 1);
        assert_eq!(v["kdf"]["alg"], "argon2id");
        assert_eq!(v["kdf"]["m_kib"], 65536);
        assert_eq!(v["aead"]["alg"], "aes-256-gcm");
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 9);
        // Typed differently: lowercase, spaces instead of dashes.
        let (h, p) = open_pack(&file, "7k4q m2zd 9xrt 4wbc").unwrap();
        assert_eq!(h, header());
        assert_eq!(p, plain);
        assert_eq!(open_pack(&file, "7K4Q-M2ZD-9XRT-4WBD"), Err(SealError::WrongCode));
    }

    #[test]
    fn envelope_field_order() {
        let file = String::from_utf8(seal_pack_with(&header(), b"x", CODE, FAST).unwrap()).unwrap();
        let pos = |k: &str| file.find(&format!("\"{k}\"")).unwrap();
        let order = ["format", "version", "mission", "cohort", "group", "pack_id", "kdf", "alg", "m_kib", "t", "p",
                     "salt", "aead", "nonce", "ciphertext"];
        for w in order.windows(2) {
            assert!(pos(w[0]) < pos(w[1]), "{} before {}", w[0], w[1]);
        }
    }

    #[test]
    fn wrong_code_and_plaintext_never_in_file() {
        let file = seal_pack_with(&header(), b"SECRET-PLAINTEXT", CODE, FAST).unwrap();
        let text = String::from_utf8(file.clone()).unwrap();
        assert!(!text.contains("SECRET-PLAINTEXT"));
        assert!(!text.contains("7K4Q"));
        assert_eq!(open_pack(&file, "AAAA-BBBB-CCCC-DDDD"), Err(SealError::WrongCode));
        assert_eq!(open_pack(&file, ""), Err(SealError::WrongCode));
        assert!(open_pack(&file, CODE).is_ok());
    }

    #[test]
    fn altered_header_field_is_wrong_code() {
        let file = seal_pack_with(&header(), b"payload", CODE, FAST).unwrap();
        let alterations: Vec<(&str, Box<dyn Fn(&mut serde_json::Value)>)> = vec![
            ("mission", Box::new(|v| v["mission"] = "demp".into())),
            ("cohort", Box::new(|v| v["cohort"] = "2026-28".into())),
            ("group", Box::new(|v| v["group"] = "G02".into())),
            ("pack_id", Box::new(|v| v["pack_id"] = "00112233445566778899aabbccddeefe".into())),
            ("m_kib", Box::new(|v| v["kdf"]["m_kib"] = 72.into())),
            ("t", Box::new(|v| v["kdf"]["t"] = 2.into())),
            ("salt", Box::new(|v| v["kdf"]["salt"] = B64.encode([7u8; 16]).into())),
            ("nonce", Box::new(|v| v["aead"]["nonce"] = B64.encode([9u8; 12]).into())),
        ];
        for (name, alter) in alterations {
            let mut v = json(&file);
            alter(&mut v);
            assert_eq!(open_pack(&bytes(&v), CODE), Err(SealError::WrongCode), "altered {name}");
        }
        // Re-serialising the untouched envelope (different whitespace) still opens: the AAD is canonical.
        assert!(open_pack(&bytes(&json(&file)), CODE).is_ok());
    }

    #[test]
    fn altered_ciphertext_byte_is_wrong_code() {
        let file = seal_pack_with(&header(), b"0123456789abcdef0123", CODE, FAST).unwrap();
        let v0 = json(&file);
        let ct = B64.decode(v0["ciphertext"].as_str().unwrap()).unwrap();
        for i in 0..ct.len() {
            let mut c = ct.clone();
            c[i] ^= 0x01;
            let mut v = v0.clone();
            v["ciphertext"] = B64.encode(&c).into();
            assert_eq!(open_pack(&bytes(&v), CODE), Err(SealError::WrongCode), "byte {i}");
        }
        let mut v = v0.clone();
        v["ciphertext"] = B64.encode(&ct[..ct.len() - 1]).into();
        assert_eq!(open_pack(&bytes(&v), CODE), Err(SealError::WrongCode));
    }

    #[test]
    fn format_errors() {
        let file = seal_pack_with(&header(), b"p", CODE, FAST).unwrap();
        let is_format = |r: Result<PackHeader, SealError>| matches!(r, Err(SealError::Format(_)));
        assert!(is_format(read_pack_header(b"not json")));
        assert!(is_format(read_pack_header(b"{}")));
        let mut v = json(&file);
        v["format"] = "other".into();
        assert!(is_format(read_pack_header(&bytes(&v))));
        let mut v = json(&file);
        v["version"] = 2.into();
        assert!(is_format(read_pack_header(&bytes(&v))));
        let mut v = json(&file);
        v["extra"] = 1.into();
        assert!(is_format(read_pack_header(&bytes(&v))));
        let mut v = json(&file);
        v["pack_id"] = "XYZ".into();
        assert!(is_format(read_pack_header(&bytes(&v))));
        let mut v = json(&file);
        v["kdf"]["m_kib"] = (MAX_M_KIB + 1).into();
        assert!(matches!(open_pack(&bytes(&v), CODE), Err(SealError::Format(_))));
        let mut v = json(&file);
        v["aead"]["nonce"] = "!!".into();
        assert!(matches!(open_pack(&bytes(&v), CODE), Err(SealError::Format(_))));
        // Messages never carry the code.
        for e in [open_pack(b"{}", CODE).unwrap_err(), open_pack(&file, "WRONG-CODE").unwrap_err()] {
            assert!(!e.to_string().contains("7K4Q") && !e.to_string().contains("WRONG"));
        }
        let bad = PackHeader { pack_id: "short".into(), ..header() };
        assert!(matches!(seal_pack_with(&bad, b"p", CODE, FAST), Err(SealError::Format(_))));
        assert!(matches!(seal_pack_with(&header(), b"p", " - ", FAST), Err(SealError::Format(_))));
    }

    #[test]
    fn header_readable_without_code() {
        let file = seal_pack_with(&header(), b"p", CODE, FAST).unwrap();
        assert_eq!(read_pack_header(&file).unwrap(), header());
    }

    #[test]
    fn code_hash_and_verify() {
        let phc = hash_code("Mon code encadrant 2026").unwrap();
        assert!(phc.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"), "{phc}");
        assert!(!phc.to_uppercase().contains("MONCODE"));
        assert!(verify_code(&phc, "Mon code encadrant 2026"));
        assert!(verify_code(&phc, "MONCODE-ENCADRANT-2026"));
        assert!(!verify_code(&phc, "Mon code encadrant 2027"));
        assert!(!verify_code(&phc, ""));
        assert_ne!(hash_code("Mon code encadrant 2026").unwrap(), phc, "random salt");
        // Garbage, truncated or foreign PHC strings.
        for bad in ["", "garbage", "$argon2id$", &phc[..phc.len() - 5], "$argon2i$v=19$m=65536,t=3,p=1$c2FsdHNhbHQ$aGFzaA",
                    "$argon2id$v=19$m=4194304,t=3,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaA"] {
            assert!(!verify_code(bad, "Mon code encadrant 2026"), "{bad}");
        }
        assert!(hash_code(" -- ").is_err());
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize_code("7k4q-m2zd 9xrt\t4wbc"), "7K4QM2ZD9XRT4WBC");
        assert_eq!(normalize_code("  ab–cd\u{a0}ef "), "ABCDEF");
        assert_eq!(normalize_code("Été"), "ÉTÉ");
        assert_eq!(normalize_code(""), "");
    }

    #[test]
    fn group_seed_is_deterministic_and_separates_length_prefixed_fields() {
        for seed in 0..32u8 {
            let master = core::array::from_fn(|i| seed.wrapping_add(i as u8));
            let group = format!("G{seed}");
            let derived = group_seed(&master, "mission", "cohort", &group);
            assert_eq!(derived, group_seed(&master, "mission", "cohort", &group));
            assert_ne!(group_seed(&master, "ab", "c", &group), group_seed(&master, "a", "bc", &group));
            assert_ne!(derived, group_seed(&master, "mission", "cohort", &format!("{group}x")));
            let mut changed = master;
            changed[usize::from(seed)] ^= 1;
            assert_ne!(derived, group_seed(&changed, "mission", "cohort", &group));
        }
    }

    #[test]
    fn sha256_has_hex_shape_and_changes_when_input_changes() {
        for len in 1..128 {
            let input: Vec<u8> = (0..len).map(|i| (i * 29 + len * 7) as u8).collect();
            let output = sha256_hex(&input);
            assert_eq!(output.len(), 64);
            assert!(output.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
            assert_eq!(output, sha256_hex(&input));
            let mut changed = input;
            changed[len / 2] ^= 1;
            assert_ne!(output, sha256_hex(&changed));
        }
    }

    #[test]
    fn group_code_format_and_alphabet() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let c = new_group_code();
            assert_eq!(c.len(), 19, "{c}");
            let parts: Vec<&str> = c.split('-').collect();
            assert_eq!(parts.len(), 4);
            assert!(parts.iter().all(|p| p.len() == 4));
            assert!(c.bytes().filter(|b| *b != b'-').all(|b| CROCKFORD.contains(&b)));
            assert!(!c.contains(['I', 'L', 'O', 'U']));
            assert_eq!(normalize_code(&c).len(), 16);
            seen.extend(c.bytes());
            assert!(seen.len() <= 33);
        }
        assert!(seen.len() > 25, "alphabet coverage");
        assert_ne!(new_group_code(), new_group_code());
    }

    #[test]
    fn random_sources() {
        assert_eq!(random_bytes(0).len(), 0);
        assert_eq!(random_bytes(33).len(), 33);
        assert_ne!(new_master_key(), new_master_key());
    }

    #[test]
    fn local_blobs_open_only_with_their_key_and_associated_data() {
        let key = [7u8; 32];
        let blob = seal_local(&key, b"bench", b"secret circuit").unwrap();
        assert_eq!(open_local(&key, b"bench", &blob).unwrap(), b"secret circuit");
        assert_ne!(seal_local(&key, b"bench", b"secret circuit").unwrap(), blob, "fresh nonce");
        assert!(!blob.windows(6).any(|w| w == b"secret"), "nothing in clear");
        assert!(open_local(&[8u8; 32], b"bench", &blob).is_err());
        assert!(open_local(&key, b"other", &blob).is_err());
        let mut bad = blob.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(open_local(&key, b"bench", &bad).is_err());
        assert!(open_local(&key, b"bench", &blob[..20]).is_err());
    }
}
