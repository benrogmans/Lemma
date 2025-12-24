use std::collections::HashMap;

use crate::FactPath;

struct InversionResponse {
    solutions: HashMap<FactPath, Domain>,
}

impl InversionResponse {
    pub fn new(solutions: HashMap<FactPath, Domain>) -> Self {
        Self { solutions }
    }
}