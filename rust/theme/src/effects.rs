//! Os efeitos do SwayFX na página de Aparência (o app Tema e a Central): a versão do SwayFX instalado, que decide se
//! a linha das animações aparece, e a fila das mudanças, que junta as de uma rajada (a SpinButton e a Scale mudam a
//! cada passo) numa só aplicação e não deixa duas correrem juntas.

/// A versão (maior, menor) da primeira linha do `sway --version` do SwayFX ("swayfx version 0.5.3"). None: não é o
/// SwayFX, ou a versão não se lê.
pub fn parse_swayfx_version(line: &str) -> Option<(u32, u32)> {
    let version = line.strip_prefix("swayfx version ")?;
    let mut parts = version.split(['.', '-', ' ']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// As mudanças esperando a aplicação, uma por chave (a última vale), e se uma aplicação está rodando.
#[derive(Debug, Default)]
pub struct EffectQueue {
    waiting: Vec<(String, String)>,
    running: bool,
}

impl EffectQueue {
    /// Uma mudança: troca a que já esperava na mesma chave.
    pub fn push(&mut self, key: &str, text: &str) {
        match self.waiting.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = text.to_string(),
            None => self.waiting.push((key.to_string(), text.to_string())),
        }
    }

    /// No fim da espera: as mudanças a aplicar, que passam a rodar. None sem nada esperando, ou com uma aplicação
    /// rodando (o fim dela, o finish, diz se é preciso chamar de novo).
    pub fn start(&mut self) -> Option<Vec<(String, String)>> {
        if self.running || self.waiting.is_empty() {
            return None;
        }
        self.running = true;
        Some(std::mem::take(&mut self.waiting))
    }

    /// O fim de uma aplicação. Devolve se chegaram mudanças enquanto ela rodava (então, um start de novo).
    pub fn finish(&mut self) -> bool {
        self.running = false;
        !self.waiting.is_empty()
    }

    /// Se há mudanças esperando ou uma aplicação rodando: enquanto isso, reler o config desfaria o que o usuário mexeu.
    pub fn busy(&self) -> bool {
        self.running || !self.waiting.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_swayfx_version_is_read_from_the_first_line() {
        assert_eq!(parse_swayfx_version("swayfx version 0.5.3"), Some((0, 5)));
        assert_eq!(parse_swayfx_version("swayfx version 0.6"), Some((0, 6)));
        assert_eq!(parse_swayfx_version("swayfx version 0.4.1-git-1234 (branch master)"), Some((0, 4)));
        assert_eq!(parse_swayfx_version("swayfx version 1.0.0"), Some((1, 0)));
        assert_eq!(parse_swayfx_version("swayfx version r123.abc"), None);
        assert_eq!(parse_swayfx_version("sway version 1.10"), None);
        assert_eq!(parse_swayfx_version(""), None);
    }

    #[test]
    fn a_burst_of_changes_is_one_apply_with_the_last_value_of_each_key() {
        let mut queue = EffectQueue::default();
        for radius in ["4", "5", "6", "7"] {
            queue.push("corner_radius", radius);
        }
        queue.push("blur", "false");
        queue.push("corner_radius", "8");
        assert!(queue.busy());
        assert_eq!(queue.start(), Some(pairs(&[("corner_radius", "8"), ("blur", "false")])));
        assert!(queue.busy());
        assert_eq!(queue.start(), None, "nothing left to start");
        assert!(!queue.finish());
        assert!(!queue.busy());
        assert_eq!(queue.start(), None);
    }

    #[test]
    fn a_change_during_an_apply_waits_for_it_to_finish() {
        let mut queue = EffectQueue::default();
        queue.push("blur", "false");
        assert_eq!(queue.start(), Some(pairs(&[("blur", "false")])));
        queue.push("shadows", "false");
        queue.push("blur", "true");
        // Duas aplicações nunca rodam juntas
        assert_eq!(queue.start(), None);
        assert!(queue.finish(), "the changes made meanwhile need another start");
        assert_eq!(queue.start(), Some(pairs(&[("shadows", "false"), ("blur", "true")])));
        assert!(!queue.finish());
    }
}
