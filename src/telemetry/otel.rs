use rand::RngCore;
use reqwest::RequestBuilder;

/// W3C Trace Context `traceparent` 头的完整二进制表示。
/// 规范：https://www.w3.org/TR/trace-context/
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceContext {
    pub version: u8,        // 固定为 0x00（当前唯一定义版本）
    pub trace_id: [u8; 16], // 128 位，全局唯一标识一条完整调用链
    pub span_id: [u8; 8],   // 64 位，标识链路中的单个操作片段
    pub trace_flags: u8,    // 目前仅定义 bit0 = sampled
}

impl TraceContext {
    /// 生成一个新的根 Trace Context（用于 ApiSnap 主动发起的请求，
    /// 当上游未传入 traceparent 时，本引擎作为链路起点）。
    pub fn new_root() -> Self {
        let mut rng = rand::thread_rng();
        let mut trace_id = [0u8; 16];
        let mut span_id = [0u8; 8];
        rng.fill_bytes(&mut trace_id);
        rng.fill_bytes(&mut span_id);

        // 规范要求 trace_id 与 span_id 不得全为零字节。
        if trace_id.iter().all(|&b| b == 0) {
            trace_id[0] = 1;
        }
        if span_id.iter().all(|&b| b == 0) {
            span_id[0] = 1;
        }

        TraceContext {
            version: 0x00,
            trace_id,
            span_id,
            trace_flags: 0x01, // sampled = true，确保回归失败链路必定被后端保留
        }
    }

    /// 从现有 trace_id 派生一个新的子 span（用于在既有调用链中标记本次校验动作）。
    pub fn new_child_span(&self) -> Self {
        let mut rng = rand::thread_rng();
        let mut span_id = [0u8; 8];
        rng.fill_bytes(&mut span_id);
        if span_id.iter().all(|&b| b == 0) {
            span_id[0] = 1;
        }
        TraceContext {
            version: self.version,
            trace_id: self.trace_id,
            span_id,
            trace_flags: self.trace_flags,
        }
    }

    /// 序列化为 `traceparent` 头的规范字符串形式：
    /// "{version}-{trace_id}-{span_id}-{trace_flags}"，全小写十六进制。
    pub fn to_traceparent_header(&self) -> String {
        format!(
            "{:02x}-{}-{}-{:02x}",
            self.version,
            hex::encode(self.trace_id),
            hex::encode(self.span_id),
            self.trace_flags
        )
    }

    /// 严格按规范解析入站 `traceparent` 头；任何字段长度/格式不合规
    /// 均返回 `None`，调用方应在解析失败时退化为 `new_root()`。
    pub fn parse(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }
        let version = u8::from_str_radix(parts[0], 16).ok()?;
        if parts[0].len() != 2 || version == 0xff {
            return None; // 0xff 版本号在规范中保留，禁止使用
        }
        if parts[1].len() != 32 || parts[2].len() != 16 || parts[3].len() != 2 {
            return None;
        }
        let trace_id_vec = hex::decode(parts[1]).ok()?;
        let span_id_vec = hex::decode(parts[2]).ok()?;
        let trace_flags = u8::from_str_radix(parts[3], 16).ok()?;

        let mut trace_id = [0u8; 16];
        let mut span_id = [0u8; 8];
        trace_id.copy_from_slice(&trace_id_vec);
        span_id.copy_from_slice(&span_id_vec);

        if trace_id.iter().all(|&b| b == 0) || span_id.iter().all(|&b| b == 0) {
            return None; // 规范禁止全零 trace_id / span_id
        }

        Some(TraceContext {
            version,
            trace_id,
            span_id,
            trace_flags,
        })
    }
}

/// 在 ApiSnap 发起的每个请求上注入 `traceparent` 头，使得本次快照测试
/// 天然成为目标微服务调用链的一部分，后端 APM 能够采集到完整的下游调用。
pub fn inject_trace_header(builder: RequestBuilder, ctx: &TraceContext) -> RequestBuilder {
    builder.header("traceparent", ctx.to_traceparent_header())
}

/// 差异发现后的根因直连：将本次比对失败关联的 trace_id 合成为
/// 目标 APM 后端的深度定位 URL，直接嵌入 DiffReport 供人工点击跳转。
#[derive(Debug, Clone)]
pub enum ApmBackend {
    Jaeger { base_url: String },
    DatadogApm { site: String }, // 例如 "datadoghq.com"
}

impl ApmBackend {
    pub fn build_trace_link(&self, ctx: &TraceContext) -> String {
        let trace_id_hex = hex::encode(ctx.trace_id);
        match self {
            ApmBackend::Jaeger { base_url } => {
                format!("{}/trace/{}", base_url.trim_end_matches('/'), trace_id_hex)
            }
            ApmBackend::DatadogApm { site } => {
                // Datadog APM 要求 trace_id 以十进制表示其低 64 位（dd_trace_id）。
                let low_64 = u64::from_be_bytes(ctx.trace_id[8..16].try_into().unwrap());
                format!("https://app.{}/apm/trace/{}", site, low_64)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_w3c_roundtrip() {
        let ctx = TraceContext::new_root();
        let header = ctx.to_traceparent_header();
        assert!(header.starts_with("00-"));

        let parsed = TraceContext::parse(&header).expect("must parse valid W3C header");
        assert_eq!(ctx.trace_id, parsed.trace_id);
        assert_eq!(ctx.span_id, parsed.span_id);
        assert_eq!(ctx.trace_flags, parsed.trace_flags);
    }

    #[test]
    fn test_apm_link_generation() {
        let ctx = TraceContext::new_root();
        let jaeger = ApmBackend::Jaeger {
            base_url: "http://localhost:16686".into(),
        };
        let link = jaeger.build_trace_link(&ctx);
        assert!(link.contains("http://localhost:16686/trace/"));

        let datadog = ApmBackend::DatadogApm {
            site: "datadoghq.com".into(),
        };
        let dd_link = datadog.build_trace_link(&ctx);
        assert!(dd_link.contains("https://app.datadoghq.com/apm/trace/"));
    }
}
