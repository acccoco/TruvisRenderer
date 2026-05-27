#include "TruvixxUtils/string.hpp"

#include <limits>

#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>

namespace truvixx::utils
{

std::string StringUtils::to_utf8(const std::wstring_view value)
{
    if (value.empty())
    {
        return {};
    }

    if (value.size() > static_cast<size_t>((std::numeric_limits<int>::max)()))
    {
        return {};
    }

    const int source_len = static_cast<int>(value.size());
    const int target_len = WideCharToMultiByte(CP_UTF8, 0, value.data(), source_len, nullptr, 0, nullptr, nullptr);
    if (target_len <= 0)
    {
        return {};
    }

    std::string result(static_cast<size_t>(target_len), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value.data(), source_len, result.data(), target_len, nullptr, nullptr);
    return result;
}

const wchar_t* StringUtils::utf16_ptr_or_default(const uint16_t* ptr, const std::wstring& fallback)
{
    static_assert(sizeof(wchar_t) == sizeof(uint16_t), "Windows wchar_t must be UTF-16.");
    return ptr ? reinterpret_cast<const wchar_t*>(ptr) : fallback.c_str();
}

} // namespace truvixx::utils
