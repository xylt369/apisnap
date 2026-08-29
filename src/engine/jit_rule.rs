use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::HashMap;

/// 编译后规则匹配函数的签名：输入指向"路径段哈希数组"的指针与其长度，
/// 输出匹配到的规则索引；未匹配时返回 u32::MAX 作为哨兵值。
pub type CompiledMatchFn = unsafe extern "C" fn(*const u64, u64) -> u32;

pub struct CraneliftRuleEngine {
    module: JITModule,
    /// 编译产物的函数指针缓存，key 为规则集的内容哈希（规则集不变时复用编译结果）。
    compiled: HashMap<u64, CompiledMatchFn>,
}

/// 路径前缀树的中间表示，编译前的规则组织形式。
#[derive(Default)]
struct PathTrieNode {
    /// 子节点：路径段的 FNV-1a 哈希 → 子 Trie 节点。
    children: HashMap<u64, PathTrieNode>,
    /// 若此节点是某条完整规则路径的终点，记录其在原始规则表中的索引。
    rule_index: Option<u32>,
}

pub fn fnv1a_hash(s: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

impl CraneliftRuleEngine {
    pub fn new() -> Self {
        let builder = JITBuilder::new(cranelift_module::default_libcall_names())
            .expect("failed to create JIT builder for target ISA");
        let module = JITModule::new(builder);
        Self {
            module,
            compiled: HashMap::new(),
        }
    }

    /// 将一组 JSONPath 规则字符串（如 "$.data.token"）编译为一个原生函数指针。
    pub fn compile_rules(&mut self, paths: &[String]) -> CompiledMatchFn {
        let rules_hash = compute_ruleset_hash(paths);
        if let Some(&cached) = self.compiled.get(&rules_hash) {
            return cached;
        }

        // 1. 构建路径前缀树
        let mut root = PathTrieNode::default();
        for (idx, path) in paths.iter().enumerate() {
            let segments: Vec<&str> = path
                .trim_start_matches('$')
                .split('.')
                .filter(|s| !s.is_empty())
                .collect();
            let mut node = &mut root;
            for seg in segments {
                let h = fnv1a_hash(seg);
                node = node.children.entry(h).or_default();
            }
            node.rule_index = Some(idx as u32);
        }

        // 2. 在 Cranelift IR 中生成函数
        let mut ctx = self.module.make_context();
        let mut fn_builder_ctx = FunctionBuilderContext::new();

        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // *const u64 数组指针
        sig.params.push(AbiParam::new(types::I64)); // 数组长度
        sig.returns.push(AbiParam::new(types::I32)); // 匹配到的规则索引

        let fn_name = format!("rule_match_{rules_hash}");
        let func_id = self
            .module
            .declare_function(&fn_name, Linkage::Export, &sig)
            .expect("failed to declare JIT function");
        ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fn_builder_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let ptr_val = builder.block_params(entry_block)[0];
            let len_val = builder.block_params(entry_block)[1];

            let no_match_block = builder.create_block();

            // 递归为 Trie 的每一层生成 Basic Block 链
            emit_trie_level(&mut builder, &root, ptr_val, len_val, 0, no_match_block);

            builder.switch_to_block(no_match_block);
            builder.seal_block(no_match_block);
            let sentinel = builder.ins().iconst(types::I32, u32::MAX as i64);
            builder.ins().return_(&[sentinel]);

            builder.finalize();
        }

        // 3. 编译并链接为可执行机器码
        self.module
            .define_function(func_id, &mut ctx)
            .expect("JIT compilation failed");
        self.module.clear_context(&mut ctx);
        self.module
            .finalize_definitions()
            .expect("JIT linking failed");

        let code_ptr = self.module.get_finalized_function(func_id);
        let compiled_fn: CompiledMatchFn = unsafe { std::mem::transmute(code_ptr) };

        self.compiled.insert(rules_hash, compiled_fn);
        compiled_fn
    }
}

/// 递归生成 Trie 某一层对应的 IR：依次对每个子分支做 `icmp eq` 判断，
/// 命中则 `brif` 跳转进入该子分支对应的下一层 Basic Block。
fn emit_trie_level(
    builder: &mut FunctionBuilder,
    node: &PathTrieNode,
    ptr_val: cranelift::prelude::Value,
    len_val: cranelift::prelude::Value,
    depth: i64,
    no_match_block: Block,
) {
    if let Some(idx) = node.rule_index {
        if node.children.is_empty() {
            let ret = builder.ins().iconst(types::I32, idx as i64);
            builder.ins().return_(&[ret]);
            return;
        }
    }

    // 边界检查：若当前深度已超出输入路径长度，直接判定未匹配。
    let depth_val = builder.ins().iconst(types::I64, depth);
    let in_bounds = builder.ins().icmp(IntCC::UnsignedLessThan, depth_val, len_val);
    let bounds_ok_block = builder.create_block();
    builder.ins().brif(in_bounds, bounds_ok_block, &[], no_match_block, &[]);
    builder.switch_to_block(bounds_ok_block);
    builder.seal_block(bounds_ok_block);

    // 从输入数组中加载当前深度的路径段哈希：*(ptr + depth * 8)
    let offset = builder.ins().iconst(types::I64, depth * 8);
    let addr = builder.ins().iadd(ptr_val, offset);
    let seg_hash = builder.ins().load(types::I64, MemFlags::new(), addr, 0);

    for (&child_hash, child_node) in node.children.iter() {
        let const_hash = builder.ins().iconst(types::I64, child_hash as i64);
        let is_eq = builder.ins().icmp(IntCC::Equal, seg_hash, const_hash);

        let match_block = builder.create_block();
        let next_check_block = builder.create_block();
        builder.ins().brif(is_eq, match_block, &[], next_check_block, &[]);

        builder.switch_to_block(match_block);
        builder.seal_block(match_block);
        emit_trie_level(builder, child_node, ptr_val, len_val, depth + 1, no_match_block);

        builder.switch_to_block(next_check_block);
        builder.seal_block(next_check_block);
    }

    // 该层所有分支哈希均未命中，跳转至未匹配终止块。
    builder.ins().jump(no_match_block, &[]);
}

fn compute_ruleset_hash(paths: &[String]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for p in paths {
        for b in p.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cranelift_jit_rule_evaluation() {
        let mut engine = CraneliftRuleEngine::new();
        let rules = vec![
            "$.user.profile.token".to_string(),
            "$.order.items.price".to_string(),
        ];

        let compiled_fn = engine.compile_rules(&rules);

        // Path 1: $.user.profile.token -> hashes: [fnv("user"), fnv("profile"), fnv("token")]
        let path1 = vec![
            fnv1a_hash("user"),
            fnv1a_hash("profile"),
            fnv1a_hash("token"),
        ];
        let res1 = unsafe { compiled_fn(path1.as_ptr(), path1.len() as u64) };
        assert_eq!(res1, 0, "must match rule 0 ($.user.profile.token)");

        // Path 2: $.order.items.price -> hashes: [fnv("order"), fnv("items"), fnv("price")]
        let path2 = vec![
            fnv1a_hash("order"),
            fnv1a_hash("items"),
            fnv1a_hash("price"),
        ];
        let res2 = unsafe { compiled_fn(path2.as_ptr(), path2.len() as u64) };
        assert_eq!(res2, 1, "must match rule 1 ($.order.items.price)");

        // Path 3: $.user.unknown -> mismatch -> u32::MAX
        let path3 = vec![fnv1a_hash("user"), fnv1a_hash("unknown")];
        let res3 = unsafe { compiled_fn(path3.as_ptr(), path3.len() as u64) };
        assert_eq!(res3, u32::MAX, "must return u32::MAX on mismatch");
    }
}
