pub mod file_summary;

use serde::Serialize;

/// 通用分页响应：列表接口统一返 `{items, total}`。
///
/// 与 HTTP 路由的 JSON 形状保持一致——前端 `apiGet` 收到的也是这两字段。
/// `limit/offset` 是输入参数，不需要回传，前端 store 自己持有当前页状态。
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
}

