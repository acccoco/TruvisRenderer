def main [repo_root: path] {
    cd $repo_root
    let metadata = (cargo metadata --no-deps --format-version 1 | from json)
    let packages = (
        $metadata.packages
        | where {|package| ($package | get -o metadata.truvis_cxx_binding) != null }
        | get name
    )
    if ($packages | is-empty) {
        error make { msg: 'No package declares package.metadata.truvis_cxx_binding' }
    }
    let package_args = ($packages | each {|package| ['-p', $package] } | flatten)
    cargo build ...$package_args
}
