use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolType {
    Function,
    Method,
    Class,
    Variable,
    Namespace,
    Local,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScipSymbol {
    pub scheme: String,
    pub package_manager: String,
    pub package_name: String,
    pub package_version: String,
    pub descriptors: Vec<Descriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Descriptor {
    pub name: String,
    pub suffix: String,
}

impl ScipSymbol {
    pub fn parse(symbol: &str) -> Option<Self> {
        if symbol.starts_with("local ") {
            return Some(Self {
                scheme: "local".to_string(),
                package_manager: "".to_string(),
                package_name: "".to_string(),
                package_version: "".to_string(),
                descriptors: vec![Descriptor {
                    name: symbol["local ".len()..].to_string(),
                    suffix: "local".to_string(),
                }],
            });
        }

        let mut parts = Vec::new();
        let mut start = 0;
        let chars: Vec<char> = symbol.chars().collect();
        let mut i = 0;
        
        while parts.len() < 4 && i < chars.len() {
            if chars[i] == ' ' {
                if i + 1 < chars.len() && chars[i + 1] == ' ' {
                    // Escaped space, skip both
                    i += 2;
                    continue;
                } else {
                    // Single space, end of part
                    let part: String = chars[start..i].iter().collect();
                    parts.push(part.replace("  ", " "));
                    start = i + 1;
                }
            }
            i += 1;
        }

        if parts.len() < 4 {
            return None;
        }

        let descriptors_str: String = chars[start..].iter().collect();
        let descriptors = parse_descriptors(&descriptors_str);

        Some(Self {
            scheme: parts[0].clone(),
            package_manager: parts[1].clone(),
            package_name: parts[2].clone(),
            package_version: parts[3].clone(),
            descriptors,
        })
    }

    pub fn name(&self) -> String {
        self.descriptors.last().map(|d| d.name.clone()).unwrap_or_default()
    }

    pub fn infer_type(&self) -> SymbolType {
        if self.scheme == "local" {
            return SymbolType::Local;
        }
        
        if let Some(last) = self.descriptors.last() {
            match last.suffix.as_str() {
                "()." => {
                    // If the parent is a class, it's a method. 
                    // For simplicity, we can just call it Method if it has ().
                    // The requirements say: infer the type (Function, Method, Class, Variable)
                    // Let's check if there's a parent descriptor that is a Class (#)
                    if self.descriptors.len() > 1 {
                        let parent = &self.descriptors[self.descriptors.len() - 2];
                        if parent.suffix == "#" {
                            return SymbolType::Method;
                        }
                    }
                    SymbolType::Function
                }
                "#" => SymbolType::Class,
                "." => SymbolType::Variable,
                "/" => SymbolType::Namespace,
                _ => SymbolType::Other,
            }
        } else {
            SymbolType::Other
        }
    }
}

fn parse_descriptors(mut s: &str) -> Vec<Descriptor> {
    let mut descriptors = Vec::new();
    while !s.is_empty() {
        // Find the next suffix
        let suffixes = ["().", "#", ".", "/", "!", ":", "[", "]", "(", ")"];
        let mut first_suffix: Option<(&str, usize)> = None;
        
        for suffix in suffixes {
            if let Some(idx) = find_unescaped_suffix(s, suffix) {
                if first_suffix.is_none() || idx < first_suffix.unwrap().1 {
                    first_suffix = Some((suffix, idx));
                }
            }
        }
        
        if let Some((suffix, idx)) = first_suffix {
            let name = s[..idx].to_string();
            // Clean up escaped backticks if necessary
            let name = name.trim_matches('`').replace("``", "`");
            descriptors.push(Descriptor {
                name,
                suffix: suffix.to_string(),
            });
            s = &s[idx + suffix.len()..];
        } else {
            // No suffix found, just add the rest as a descriptor with no suffix?
            // This shouldn't happen in a valid SCIP symbol.
            break;
        }
    }
    descriptors
}

fn find_unescaped_suffix(s: &str, suffix: &str) -> Option<usize> {
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(suffix) {
            // Check if it's inside backticks
            let before = &s[..i];
            let backtick_count = before.chars().filter(|&c| c == '`').count();
            if backtick_count % 2 == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}
