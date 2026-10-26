#pragma once

namespace truvixx::utils
{
/// Streamline 与直接 NGX 调用共享同一项目身份；不借用第三方产品的 application ID。
struct NgxIdentity
{
    static constexpr const char* project_id = "e390aa77-d1d4-42c5-be02-07095506af72";
    static constexpr const char* engine_version = "Truvis";
};
} // namespace truvixx::utils
