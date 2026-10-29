//! Whisper's tokens made into words.
//!
//! Whisper hears in tokens - pieces of words, as bytes - with a time and
//! a probability each. A word starts at a token that starts with a space;
//! the tokens after it, up to the next space, are the rest of it, trailing
//! punctuation included. Bytes are joined before they are read as text: a
//! token can end halfway through an "ñ".
//!
//! Times come from the alignment (DTW) when whisper ran it: a word starts
//! where its first token is aligned and ends where the token after it is,
//! as faster-whisper times words. Without it, from whisper's own guesses.

use crate::{PalabraOida, Tramo};

/// whisper.cpp's alignment puts a token 0.13 to 0.19 s after it is heard
/// to start (measured against a take's loudness, word by word). Aligned
/// times are moved this much earlier, in centiseconds, so that a cut on a
/// word's start never clips its first sound.
const ADELANTO_ALINEADO: i64 = 20;

/// One token as whisper gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct TokenOido {
    /// Its bytes, not always whole characters.
    pub bytes: Vec<u8>,
    /// Whisper's probability for it, 0 to 1.
    pub probabilidad: f64,
    /// Its log-probability.
    pub logprob: f64,
    /// Start, in whisper's centiseconds.
    pub t0: i64,
    /// End, in whisper's centiseconds.
    pub t1: i64,
    /// Where the alignment put it, in centiseconds, when it ran.
    pub alineado: Option<i64>,
    /// A timestamp or control token, not speech.
    pub especial: bool,
}

/// A segment's speech tokens grouped into words, each with the mean of
/// its tokens' probabilities.
pub fn palabras_de(tokens: &[TokenOido]) -> Vec<PalabraOida> {
    grupos(tokens)
        .into_iter()
        .filter_map(|grupo| palabra(tokens, grupo))
        .collect()
}

/// How far a segment's speech can be trusted, for [`crate::idioma::confianza`].
pub fn tramo(tokens: &[TokenOido]) -> Tramo {
    let habla: Vec<&TokenOido> = tokens.iter().filter(|token| !token.especial).collect();
    let logprob_medio = if habla.is_empty() {
        0.0
    } else {
        habla.iter().map(|token| token.logprob).sum::<f64>() / habla.len() as f64
    };
    Tramo {
        logprob_medio,
        palabras: palabras_de(tokens).len(),
    }
}

/// Each word's tokens, as index ranges into `tokens`.
fn grupos(tokens: &[TokenOido]) -> Vec<std::ops::Range<usize>> {
    let mut grupos: Vec<std::ops::Range<usize>> = Vec::new();
    for (indice, token) in tokens.iter().enumerate().filter(|(_, token)| !token.especial) {
        match grupos.last_mut() {
            Some(grupo) if grupo.end == indice && !token.bytes.starts_with(b" ") => grupo.end += 1,
            _ => grupos.push(indice..indice + 1),
        }
    }
    grupos
}

fn palabra(tokens: &[TokenOido], grupo: std::ops::Range<usize>) -> Option<PalabraOida> {
    let suyos = &tokens[grupo.clone()];
    let bytes: Vec<u8> = suyos.iter().flat_map(|token| token.bytes.iter().copied()).collect();
    let texto = String::from_utf8_lossy(&bytes).into_owned();
    if texto.trim().is_empty() {
        return None;
    }
    let (primero, ultimo) = (suyos.first()?, suyos.last()?);
    let siguiente = tokens.get(grupo.end);
    let inicio = alineado(primero).unwrap_or(primero.t0);
    let fin = siguiente.and_then(alineado).unwrap_or(ultimo.t1).max(inicio);
    Some(PalabraOida {
        inicio: inicio as f64 / 100.0,
        fin: fin as f64 / 100.0,
        texto,
        probabilidad: suyos.iter().map(|token| token.probabilidad).sum::<f64>() / suyos.len() as f64,
    })
}

/// Where the alignment heard `token` start, brought forward by its lag.
fn alineado(token: &TokenOido) -> Option<i64> {
    token.alineado.map(|t| (t - ADELANTO_ALINEADO).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(bytes: &[u8], t0: i64, probabilidad: f64) -> TokenOido {
        TokenOido {
            bytes: bytes.to_vec(),
            probabilidad,
            logprob: probabilidad.ln(),
            t0,
            t1: t0 + 10,
            alineado: None,
            especial: false,
        }
    }

    fn con_tiempo(bytes: &[u8], t: i64) -> TokenOido {
        TokenOido { alineado: Some(t), ..token(bytes, 0, 0.9) }
    }

    fn especial(t0: i64) -> TokenOido {
        TokenOido { especial: true, ..token(b"[_TT_]", t0, 1.0) }
    }

    #[test]
    fn tokens_after_a_space_start_a_new_word() {
        let tokens = [especial(0), token(b" Pa", 0, 0.9), token(b"usoz", 10, 0.7), token(b" pauso", 20, 0.8), token(b".", 30, 1.0)];
        let palabras = palabras_de(&tokens);
        let textos: Vec<&str> = palabras.iter().map(|p| p.texto.as_str()).collect();
        assert_eq!(textos, [" Pausoz", " pauso."]);
        assert_eq!((palabras[0].inicio, palabras[0].fin), (0.0, 0.2));
        assert!((palabras[0].probabilidad - 0.8).abs() < 1e-12);
    }

    #[test]
    fn a_character_split_across_tokens_is_whole_again() {
        let enye = "ñ".as_bytes();
        let tokens = [token(b" ma", 0, 0.9), token(&enye[..1], 10, 0.9), token(&[enye[1], b'a'], 20, 0.9), token(b"na", 30, 0.9)];
        assert_eq!(palabras_de(&tokens)[0].texto, " mañana");
    }

    #[test]
    fn aligned_words_run_to_the_next_token() {
        let cierre = TokenOido { especial: true, ..con_tiempo(b"[_TT_]", 189) };
        let tokens = [con_tiempo(b" Hoy", 121), con_tiempo(b" qui", 163), con_tiempo(b"ero", 175), cierre];
        let palabras = palabras_de(&tokens);
        assert_eq!((palabras[0].inicio, palabras[0].fin), (1.01, 1.43), "a fifth of a second early");
        assert_eq!((palabras[1].inicio, palabras[1].fin), (1.43, 1.69), "the closing timestamp token ends it");
    }

    #[test]
    fn a_word_never_ends_before_it_starts_nor_before_the_sound() {
        let tokens = [con_tiempo(b" s\xc3\xad", 300), con_tiempo(b" no", 290), con_tiempo(b" ya", 10)];
        let palabras = palabras_de(&tokens);
        assert_eq!((palabras[0].inicio, palabras[0].fin), (2.8, 2.8));
        assert_eq!(palabras[2].inicio, 0.0);
    }

    #[test]
    fn a_segment_without_speech_has_no_words() {
        assert!(palabras_de(&[especial(0), especial(10)]).is_empty());
        assert!(palabras_de(&[token(b" ", 0, 0.5)]).is_empty());
        assert_eq!(tramo(&[especial(0)]), Tramo { logprob_medio: 0.0, palabras: 0 });
    }

    #[test]
    fn a_segment_is_trusted_by_its_speech_tokens() {
        let tokens = [especial(0), token(b" Hoy", 0, 0.5), token(b" s\xc3\xad", 10, 0.5)];
        let tramo = tramo(&tokens);
        assert_eq!(tramo.palabras, 2);
        assert!((tramo.logprob_medio - 0.5f64.ln()).abs() < 1e-12);
    }
}
