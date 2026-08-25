/// 定义一个 macro，自动为各种派生 Buffer 类型实现 Deref、DerefMut 和
/// GfxDebugType
#[macro_export]
macro_rules! impl_derive_buffer {
    // 支持泛型的版本
    ($name:ident<$($generic:ident $(: $bound:path)?),*>, $target:ty, $inner:ident) => {
        impl<$($generic $(: $bound)?),*> Deref for $name<$($generic),*> {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.$inner
            }
        }

        impl<$($generic $(: $bound)?),*> DerefMut for $name<$($generic),*> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.$inner
            }
        }

        impl<$($generic $(: $bound)?),*> $name<$($generic),*> {
            pub fn destroy_mut(
                &mut self,
                ctx: $crate::gfx::GfxResourceCtx<'_>,
                reason: $crate::resources::lifecycle::DestroyReason,
            ) {
                self.$inner.destroy_mut(ctx, reason);
            }

            pub fn destroy(
                self,
                ctx: $crate::gfx::GfxResourceCtx<'_>,
                reason: $crate::resources::lifecycle::DestroyReason,
            ) {
                self.$inner.destroy(ctx, reason);
            }
        }
    };
    // 非泛型版本
    ($name:ident, $target:ty, $inner:ident) => {
        impl Deref for $name {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.$inner
            }
        }

        impl DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.$inner
            }
        }

        impl $name {
            pub fn destroy_mut(
                &mut self,
                ctx: $crate::gfx::GfxResourceCtx<'_>,
                reason: $crate::resources::lifecycle::DestroyReason,
            ) {
                self.$inner.destroy_mut(ctx, reason);
            }

            pub fn destroy(
                self,
                ctx: $crate::gfx::GfxResourceCtx<'_>,
                reason: $crate::resources::lifecycle::DestroyReason,
            ) {
                self.$inner.destroy(ctx, reason);
            }
        }
    };
}
