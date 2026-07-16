/// highlight.js 单例 + 注册项目里用到的两种语言。
///
/// 暴露给 `<n-config-provider :hljs="hljs">`；不绑的话 Naive UI 的
/// `<n-code>` 在 dev 模式每渲染一次就 `hljs is not set` 警告一次。
///
/// 只装 bash + json：项目里 cURL 块和 JSON 响应体分别用这两个。
/// `lib/core` 比 `lib/common` 小很多（common 带 30+ 语言）。
import hljs from "highlight.js/lib/core"
import bash from "highlight.js/lib/languages/bash"
import json from "highlight.js/lib/languages/json"

hljs.registerLanguage("bash", bash)
hljs.registerLanguage("json", json)

export default hljs
