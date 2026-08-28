#include "TruvixxUtils/path.hpp"

#include "TruvixxUtils/string.hpp"

#include <filesystem>

#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>

namespace truvixx::utils
{

std::wstring PathUtils::current_executable_dir()
{
    std::wstring buffer(MAX_PATH, L'\0');

    for (;;)
    {
        const DWORD len = GetModuleFileNameW(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
        if (len == 0)
        {
            return L".";
        }

        if (len < buffer.size())
        {
            buffer.resize(len);
            return std::filesystem::path(buffer).parent_path().wstring();
        }

        buffer.resize(buffer.size() * 2);
    }
}

std::wstring PathUtils::default_temp_streamline_log_dir()
{
    return (std::filesystem::current_path() / ".temp" / "streamline").wstring();
}

bool PathUtils::ensure_directory(const wchar_t* path, std::string* error_message)
{
    if (!path || path[0] == L'\0')
    {
        return true;
    }

    std::error_code ec;
    std::filesystem::create_directories(std::filesystem::path(path), ec);
    if (!ec)
    {
        return true;
    }

    if (error_message)
    {
        *error_message = "failed to create directory: " + StringUtils::to_utf8(path) + ", " + ec.message();
    }
    return false;
}

} // namespace truvixx::utils
