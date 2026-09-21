def main [repo_root: path, bin: string, ...run_opts: string] {
    let validation_settings = ($repo_root | path join 'config' 'vulkan' 'khronos_validation_settings.txt')
    let is_truvis_bin = ($bin == 'truvis-app')
    let enable_imgui = ($run_opts | any {|opt| $opt == 'imgui' })
    let enable_validation = not ($run_opts | any {|opt| $opt == 'no-validation' })
    let app_args = ($run_opts | where {|opt| $opt != 'imgui' and $opt != 'no-validation' })

    cd $repo_root
    if $is_truvis_bin {
        $env.TRUVIS_STREAMLINE_IMGUI = if $enable_imgui { '1' } else { '0' }
    }
    if $enable_validation {
        $env.VK_LOADER_LAYERS_ENABLE = 'VK_LAYER_KHRONOS_validation'
        $env.VK_LAYER_SETTINGS_PATH = $validation_settings
    }
    cargo run --bin $bin -- ...$app_args
}
