use blake3::Hash;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 256 位内容哈希的强类型包装，避免与普通 [u8; 32] 混用出错。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeHash(pub [u8; 32]);

impl NodeHash {
    pub fn from_blake3(h: Hash) -> Self {
        NodeHash(*h.as_bytes())
    }

    /// 用于磁盘分块目录布局：取前 2 字节十六进制作为分片目录，
    /// 避免单一目录下堆积百万级小文件导致的文件系统查找退化。
    pub fn shard_path(&self, root: &Path) -> PathBuf {
        let hex_str = hex::encode(self.0);
        root.join(&hex_str[0..2]).join(&hex_str[2..])
    }
}

impl AsRef<[u8]> for NodeHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// AST 节点的 Merkle 化表示，替代裸 `serde_json::Value` 作为落盘单元。
/// 每个变体只存储"直接子节点的哈希"而非子节点本体，实现结构共享。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MerkleNode {
    Null,
    Bool(bool),
    /// 存储原始规范化字节而非 f64，避免序列化精度损失导致哈希漂移。
    Number { canonical_bytes: Vec<u8> },
    String(String),
    Array { children: Vec<NodeHash> },
    /// 键已按字节序排序存储，与哈希计算顺序保持一致。
    Object { entries: Vec<(String, NodeHash)> },
}

/// 单个 CAS 对象在磁盘上的物理表示：内容寻址，键即是内容哈希。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CasObject {
    pub hash: NodeHash,
    pub node: MerkleNode,
}

/// 内容寻址存储主体：负责建树、去重落盘、按哈希取回。
pub struct MerkleCasStore {
    /// CAS 对象根目录，例如 `__snapshots__/.cas/`。
    root_dir: PathBuf,
    /// 进程内 LRU 缓存，避免同一 CAS 运行中重复读盘反序列化。
    cache: HashMap<NodeHash, MerkleNode>,
}

impl MerkleCasStore {
    pub fn new<P: AsRef<Path>>(root_dir: P) -> std::io::Result<Self> {
        let path = root_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&path)?;
        Ok(Self {
            root_dir: path,
            cache: HashMap::new(),
        })
    }

    /// 递归建树：输入原始（已脱敏）JSON AST，输出根哈希；
    /// 沿途每个子树若尚未存在于 CAS 中，则写盘一次；已存在的子树直接复用，
    /// 这正是去重发生的地方——写盘调用次数等于"新增/变更子树数"，而非总节点数。
    pub fn ingest(&mut self, value: &Value) -> std::io::Result<NodeHash> {
        let node = match value {
            Value::Null => MerkleNode::Null,
            Value::Bool(b) => MerkleNode::Bool(*b),
            Value::Number(n) => MerkleNode::Number {
                canonical_bytes: canonicalize_number(n),
            },
            Value::String(s) => MerkleNode::String(s.clone()),
            Value::Array(arr) => {
                let mut children = Vec::with_capacity(arr.len());
                for elem in arr {
                    children.push(self.ingest(elem)?); // 后序：先递归子节点
                }
                MerkleNode::Array { children }
            }
            Value::Object(map) => {
                let mut entries: Vec<(String, NodeHash)> = Vec::with_capacity(map.len());
                for (k, v) in map {
                    entries.push((k.clone(), self.ingest(v)?)); // 后序：先递归子节点
                }
                entries.sort_by(|a, b| a.0.cmp(&b.0)); // 按键排序，见 1.1 数学定义
                MerkleNode::Object { entries }
            }
        };

        let hash = self.compute_hash(&node);
        self.write_if_absent(hash, &node)?;
        Ok(hash)
    }

    pub fn compute_hash(&self, node: &MerkleNode) -> NodeHash {
        let mut hasher = blake3::Hasher::new();
        match node {
            MerkleNode::Null => hasher.update(&[0u8]),
            MerkleNode::Bool(b) => hasher.update(&[1u8, *b as u8]),
            MerkleNode::Number { canonical_bytes } => {
                hasher.update(&[2u8]);
                hasher.update(canonical_bytes);
                &mut hasher
            }
            MerkleNode::String(s) => {
                hasher.update(&[3u8]);
                hasher.update(s.as_bytes());
                &mut hasher
            }
            MerkleNode::Array { children } => {
                hasher.update(&[4u8]);
                hasher.update(&(children.len() as u32).to_le_bytes());
                for child_hash in children {
                    hasher.update(&child_hash.0);
                }
                &mut hasher
            }
            MerkleNode::Object { entries } => {
                hasher.update(&[5u8]);
                hasher.update(&(entries.len() as u32).to_le_bytes());
                for (key, val_hash) in entries {
                    hasher.update(blake3::hash(key.as_bytes()).as_bytes());
                    hasher.update(&val_hash.0);
                }
                &mut hasher
            }
        };
        NodeHash::from_blake3(hasher.finalize())
    }

    /// 分块去重落盘：若目标哈希对应的文件已存在，直接跳过写入（相同内容永远只占用一份磁盘 inode）。
    fn write_if_absent(&mut self, hash: NodeHash, node: &MerkleNode) -> std::io::Result<()> {
        if self.cache.contains_key(&hash) {
            return Ok(());
        }
        let path = hash.shard_path(&self.root_dir);
        if path.exists() {
            self.cache.insert(hash, node.clone());
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let obj = CasObject {
            hash,
            node: node.clone(),
        };
        let bytes = bincode::serialize(&obj).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("bincode serialization error: {e}"))
        })?;

        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, &bytes)?;
        std::fs::rename(&tmp_path, &path)?;
        self.cache.insert(hash, node.clone());
        Ok(())
    }

    /// 反序列化还原：从根哈希递归重建完整 `serde_json::Value`。
    pub fn reconstruct(&mut self, hash: NodeHash) -> std::io::Result<Value> {
        let node = self.load(hash)?;
        Ok(match node {
            MerkleNode::Null => Value::Null,
            MerkleNode::Bool(b) => Value::Bool(b),
            MerkleNode::Number { canonical_bytes } => {
                Value::Number(decanonicalize_number(&canonical_bytes))
            }
            MerkleNode::String(s) => Value::String(s),
            MerkleNode::Array { children } => {
                let mut arr = Vec::with_capacity(children.len());
                for child_hash in children {
                    arr.push(self.reconstruct(child_hash)?);
                }
                Value::Array(arr)
            }
            MerkleNode::Object { entries } => {
                let mut map = serde_json::Map::new();
                for (k, val_hash) in entries {
                    map.insert(k, self.reconstruct(val_hash)?);
                }
                Value::Object(map)
            }
        })
    }

    pub fn load(&mut self, hash: NodeHash) -> std::io::Result<MerkleNode> {
        if let Some(n) = self.cache.get(&hash) {
            return Ok(n.clone());
        }
        let path = hash.shard_path(&self.root_dir);
        let bytes = std::fs::read(&path)?;
        let obj: CasObject = bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.cache.insert(hash, obj.node.clone());
        Ok(obj.node)
    }
}

fn canonicalize_number(n: &serde_json::Number) -> Vec<u8> {
    if let Some(i) = n.as_i64() {
        let mut v = vec![0u8]; // 判别符：整数
        v.extend_from_slice(&i.to_be_bytes());
        v
    } else {
        let f = n.as_f64().unwrap_or(0.0);
        let mut v = vec![1u8]; // 判别符：浮点数
        v.extend_from_slice(&f.to_be_bytes());
        v
    }
}

fn decanonicalize_number(bytes: &[u8]) -> serde_json::Number {
    if bytes.is_empty() {
        return serde_json::Number::from(0);
    }
    match bytes[0] {
        0 if bytes.len() >= 9 => {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[1..9]);
            serde_json::Number::from(i64::from_be_bytes(arr))
        }
        _ if bytes.len() >= 9 => {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[1..9]);
            serde_json::Number::from_f64(f64::from_be_bytes(arr))
                .unwrap_or_else(|| serde_json::Number::from(0))
        }
        _ => serde_json::Number::from(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_merkle_cas_roundtrip_and_deduplication() {
        let tmp = tempdir().unwrap();
        let mut store = MerkleCasStore::new(tmp.path()).unwrap();

        let val1 = json!({
            "user": {
                "name": "Alice",
                "roles": ["admin", "editor"]
            },
            "status": "active"
        });

        let hash1 = store.ingest(&val1).unwrap();
        let restored1 = store.reconstruct(hash1).unwrap();
        assert_eq!(val1, restored1);

        // Val2 only changes status -> "inactive"
        let val2 = json!({
            "user": {
                "name": "Alice",
                "roles": ["admin", "editor"]
            },
            "status": "inactive"
        });

        let hash2 = store.ingest(&val2).unwrap();
        assert_ne!(hash1, hash2);

        let restored2 = store.reconstruct(hash2).unwrap();
        assert_eq!(val2, restored2);

        // The user sub-tree in val1 and val2 shares the exact same MerkleNode & hash
        let user_hash1 = match store.load(hash1).unwrap() {
            MerkleNode::Object { entries } => entries.iter().find(|(k, _)| k == "user").unwrap().1,
            _ => panic!("expected object"),
        };

        let user_hash2 = match store.load(hash2).unwrap() {
            MerkleNode::Object { entries } => entries.iter().find(|(k, _)| k == "user").unwrap().1,
            _ => panic!("expected object"),
        };

        assert_eq!(user_hash1, user_hash2, "User subtree hash must be identical and deduplicated on disk");
    }
}
