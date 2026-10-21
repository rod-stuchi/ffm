//! Fictional names — one map for every text the household typed.
//!
//! A title like `Energia · Enel SP` is split on ` · ` and each part mapped on
//! its own, case-insensitively, so the bill name `Enel SP` and that title
//! keep agreeing, and so does every old title in the change log. A fake is
//! never an original, never contains one, and is never given to two
//! originals.
//!
//! The pools are Brazilian-household Portuguese on purpose: this is content a
//! household typed into its own ledger, the same as the seed data
//! (CLAUDE.md, Conventions) — never app copy, never in a catalog.

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pool {
    Bucket,
    /// Whoever issues a recurring bill.
    Company,
    /// A one-off spend.
    Spend,
    Income,
    Instance,
    Person,
    Household,
}

const BUCKETS: &[&str] = &[
    "Moradia",
    "Consumo",
    "Transporte",
    "Saúde",
    "Educação",
    "Lazer",
    "Alimentação",
    "Assinaturas",
    "Impostos",
    "Seguros",
    "Pets",
    "Vestuário",
    "Viagens",
    "Investimentos",
    "Doações",
    "Manutenção",
    "Serviços",
    "Filhos",
    "Cuidados",
    "Utilidades",
    "Tributos",
    "Mobilidade",
    "Comunicação",
    "Bem-estar",
    "Hobbies",
    "Reformas",
    "Presentes",
];

const COMPANY_HEADS: &[&str] = &[
    "Águas",
    "Luz",
    "Gás",
    "Fibra",
    "Tele",
    "Banco",
    "Card",
    "Seguradora",
    "Auto",
    "Rede",
    "Vida",
    "Prev",
    "Conecta",
    "Força",
    "Saneamento",
    "Crédito",
    "Mobi",
    "Clube",
    "Condomínio",
    "Prefeitura de",
];

const COMPANY_TAILS: &[&str] = &[
    "do Vale",
    "Serra Clara",
    "Paulista",
    "do Sul",
    "Norte",
    "Mais",
    "Brasil",
    "Central",
    "Prime",
    "Litoral",
    "Azul",
    "Horizonte",
    "Planalto",
    "Real",
    "das Flores",
    "Bela Vista",
];

const SPENDS: &[&str] = &[
    "Padaria",
    "Feira",
    "Farmácia",
    "Açougue",
    "Posto",
    "Restaurante",
    "Cinema",
    "Presente",
    "Livraria",
    "Pet shop",
    "Sorveteria",
    "Papelaria",
    "Lavanderia",
    "Barbearia",
    "Hortifruti",
    "Lanchonete",
    "Pizzaria",
    "Estacionamento",
    "Pedágio",
    "Academia",
    "Floricultura",
    "Sapataria",
    "Ótica",
    "Chaveiro",
    "Marmitaria",
    "Doceria",
    "Empório",
    "Quitanda",
    "Mercearia",
    "Bazar",
    "Conserto",
    "Corrida de app",
    "Táxi",
    "Ingresso",
    "Revista",
    "Café",
    "Brinquedo",
    "Material escolar",
    "Remédio",
    "Costureira",
    "Encanador",
    "Eletricista",
];

const INCOMES: &[&str] = &[
    "Salário",
    "Freelance",
    "Aluguel recebido",
    "Reembolso",
    "Dividendos",
    "Bônus",
    "Consultoria",
    "Venda",
    "Aulas particulares",
    "Pró-labore",
    "Comissão",
    "13º salário",
    "Férias",
    "Restituição",
    "Cashback",
    "Rendimento",
    "Pensão",
    "Bico",
    "Prêmio",
    "Juros",
];

const INSTANCES: &[&str] = &[
    "Casa 12",
    "Apto 31",
    "Sítio",
    "Chácara",
    "Loja",
    "Escritório",
    "Casa da praia",
    "Apto 204",
    "Galpão",
    "Sala 5",
    "Casa 7",
    "Edícula",
];

const PEOPLE: &[&str] = &[
    "ana", "bruno", "carla", "diego", "elisa", "fábio", "gabriela", "heitor", "isabel", "joão",
    "karina", "lucas", "marina", "nicolas", "olívia", "paulo", "rafael", "sofia", "tiago", "vitor",
    "yasmin", "zeca", "beatriz", "caio", "denise", "eduardo",
];

const SURNAMES: &[&str] = &[
    "Almeida",
    "Barros",
    "Cardoso",
    "Duarte",
    "Esteves",
    "Farias",
    "Gouveia",
    "Holanda",
    "Junqueira",
    "Lacerda",
    "Moraes",
    "Nogueira",
    "Pacheco",
    "Queiroz",
    "Rezende",
    "Siqueira",
    "Teixeira",
    "Vasconcelos",
];

pub struct Names {
    map: HashMap<String, String>,
    used: HashSet<String>,
    /// Every original, lowercased — what a fake may neither be nor contain.
    forbidden: Vec<String>,
    pools: HashMap<Pool, Vec<String>>,
    next: HashMap<Pool, usize>,
}

impl Names {
    pub fn new(rng: &mut StdRng, originals: &HashSet<String>) -> Names {
        let own = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut companies = Vec::new();
        for h in COMPANY_HEADS {
            for t in COMPANY_TAILS {
                companies.push(format!("{h} {t}"));
            }
        }
        let mut households = Vec::new();
        for s in SURNAMES {
            for h in ["Casa", "Família", "Lar"] {
                households.push(format!("{h} {s}"));
            }
        }
        let mut pools = HashMap::from([
            (Pool::Bucket, own(BUCKETS)),
            (Pool::Company, companies),
            (Pool::Spend, own(SPENDS)),
            (Pool::Income, own(INCOMES)),
            (Pool::Instance, own(INSTANCES)),
            (Pool::Person, own(PEOPLE)),
            (Pool::Household, households),
        ]);
        // A fixed order before shuffling, so the seed alone decides.
        let mut keys: Vec<Pool> = pools.keys().copied().collect();
        keys.sort_by_key(|p| *p as u8);
        for k in keys {
            pools.get_mut(&k).unwrap().shuffle(rng);
        }
        let mut forbidden: Vec<String> = originals
            .iter()
            .map(|s| s.to_lowercase())
            .filter(|s| s.chars().count() >= 4)
            .collect();
        forbidden.sort();
        Names {
            map: HashMap::new(),
            used: HashSet::new(),
            forbidden,
            pools,
            next: HashMap::new(),
        }
    }

    /// Maps every ` · `-separated part of `s`.
    pub fn map(&mut self, s: &str, pool: Pool) -> String {
        s.split(" · ")
            .map(|part| self.part(part, pool))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn part(&mut self, part: &str, pool: Pool) -> String {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return part.to_string();
        }
        let key = trimmed.to_lowercase();
        if let Some(v) = self.map.get(&key) {
            return v.clone();
        }
        let fake = self.fresh(pool);
        self.used.insert(fake.to_lowercase());
        self.map.insert(key, fake.clone());
        fake
    }

    /// What `part` was already mapped to, if anything.
    pub fn known(&self, part: &str) -> Option<&String> {
        self.map.get(&part.trim().to_lowercase())
    }

    fn fresh(&mut self, pool: Pool) -> String {
        let items = &self.pools[&pool];
        let mut round = 0usize;
        loop {
            let i = self.next.entry(pool).or_insert(0);
            let base = &items[*i % items.len()];
            round = round.max(*i / items.len());
            *i += 1;
            let candidate = if round == 0 {
                base.clone()
            } else {
                format!("{base} {}", roman(round + 1))
            };
            let lower = candidate.to_lowercase();
            if !self.used.contains(&lower)
                && !self.forbidden.iter().any(|f| lower.contains(f.as_str()))
            {
                return candidate;
            }
            if *self.next.get(&pool).unwrap() > items.len() * 40 {
                // Every candidate clashes with an original: number them.
                return format!("{base} {}", self.used.len() + 1);
            }
        }
    }

    /// Every original part and what it became, longest first — for replacing
    /// inside free text (a change log note).
    pub fn pairs(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        v.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        v
    }
}

fn roman(n: usize) -> &'static str {
    [
        "", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X",
    ]
    .get(n)
    .copied()
    .unwrap_or("X")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn parts_agree_across_fields() {
        let mut rng = StdRng::seed_from_u64(4);
        let originals = HashSet::from(["Energia".to_string(), "Enel SP".to_string()]);
        let mut n = Names::new(&mut rng, &originals);
        let bill = n.map("Enel SP", Pool::Company);
        let title = n.map("Energia · Enel SP", Pool::Company);
        assert!(title.ends_with(&format!(" · {bill}")));
        assert_eq!(n.map("enel sp", Pool::Company), bill);
    }

    #[test]
    fn a_fake_never_contains_an_original() {
        let mut rng = StdRng::seed_from_u64(4);
        let originals: HashSet<String> = ["Padaria", "Moradia", "Banco"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut n = Names::new(&mut rng, &originals);
        for i in 0..60 {
            let f = n.map(&format!("x{i}"), Pool::Company).to_lowercase();
            assert!(!f.contains("banco"), "{f}");
        }
        for i in 0..30 {
            let f = n.map(&format!("y{i}"), Pool::Bucket).to_lowercase();
            assert!(!f.contains("moradia"), "{f}");
        }
    }
}
