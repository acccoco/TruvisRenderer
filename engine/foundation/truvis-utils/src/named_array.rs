/// 创建一个带有索引的常量数组
#[macro_export]
macro_rules! enumed_map {
    ($enum_name:ident<$vtype:ty>: { $($variant:ident: $value:expr),* $(,)? }) => {
        // 定义枚举
        #[repr(usize)]
        #[derive(Debug, Clone, Copy)]
        enum $enum_name {
            $($variant,)*
        }

        // 定义索引方法
        impl $enum_name {
            const COUNT: usize = count_indexed_array!($($variant),*);

            // 获取数组的静态方法
            fn get_array() -> &'static [$vtype; Self::COUNT] {
                // OnceLock 的开销：get 大约是 1~3 cycles
                // 使用 OnceLock 实现延迟初始化的静态数组
                static ARRAY: std::sync::OnceLock<[$vtype; { count_indexed_array!($($variant),*) }]> = std::sync::OnceLock::new();

                ARRAY.get_or_init(|| [
                    $($value,)*
                ])
            }

            pub fn value(self) -> &'static $vtype {
                &Self::get_array()[self as usize]
            }

            pub const fn index(self) -> usize {
                self as usize
            }

            pub fn iter() -> impl Iterator<Item = Self> {
                (0..Self::COUNT).map(|i| unsafe { std::mem::transmute(i) })
            }

            // 提供访问整个数组的静态方法
            pub fn array() -> &'static [$vtype; Self::COUNT] {
                Self::get_array()
            }
        }
    };
}

/// 辅助宏，计算变体数量
#[macro_export]
macro_rules! count_indexed_array {
    () => (0);
    ($head:tt $(, $tail:tt)*) => (1 + count_indexed_array!($($tail),*));
}
