use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Fields, Meta, parse_macro_input};

/// 为结构体实现 DescriptorBinding 派生宏
///
/// 支持的属性：
/// - binding: 指定绑定点编号
/// - descriptor_type: 指定描述符类型（如 UNIFORM_BUFFER, COMBINED_IMAGE_SAMPLER 等）
/// - count: 指定描述符数量
/// - stage: 指定着色器阶段（如 VERTEX, FRAGMENT 等）
/// - flags: 指定描述符绑定标志（如 UPDATE_AFTER_BIND, PARTIALLY_BOUND 等）
#[proc_macro_derive(DescriptorBinding, attributes(binding, descriptor_type, count, stage, flags))]
pub fn derive_descriptor_binding(input: TokenStream) -> TokenStream {
    // 解析输入为 DeriveInput 结构
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    // 只处理结构体类型，且只支持具名字段
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Only named fields are supported"),
        },
        _ => panic!("Only structs are supported"),
    };

    // 收集字段信息：名称、绑定、描述符类型、数量、着色器阶段和标志
    let mut field_infos = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let binding = get_binding_value(&field.attrs);
        let descriptor_type = get_descriptor_type(&field.attrs);
        let count = get_count_value(&field.attrs);
        let stage = get_stage_value(&field.attrs);
        let flags = get_flags_value(&field.attrs);

        if let Some(binding) = binding {
            // 创建不带前后缀下划线的方法名
            let method_name = syn::Ident::new(field_name.to_string().trim_matches('_'), field_name.span());
            field_infos.push((field_name, method_name, binding, descriptor_type, count, stage, flags));
        }
    }

    // 生成获取绑定信息的方法
    let field_names = field_infos.iter().map(|(name, ..)| name).collect::<Vec<_>>();
    let method_names = field_infos.iter().map(|(_, method_name, ..)| method_name).collect::<Vec<_>>();
    let binding_values = field_infos.iter().map(|(_, _, binding, ..)| binding).collect::<Vec<_>>();
    let descriptor_types = field_infos.iter().map(|(_, _, _, descriptor_type, ..)| descriptor_type).collect::<Vec<_>>();
    let counts = field_infos.iter().map(|(.., count, _, _)| count).collect::<Vec<_>>();
    let stages = field_infos.iter().map(|(.., stage, _)| stage).collect::<Vec<_>>();
    let flags = field_infos.iter().map(|(.., flags)| flags).collect::<Vec<_>>();

    // 生成代码：
    // 1. 实现 get_shader_bindings 方法，返回字段名和绑定值的元组数组
    // 2. 实现 DescriptorBindingLayout trait，返回完整的 DescriptorBindingItem 数组
    let expanded = quote! {
        impl #struct_name {
            #(
                pub fn #method_names() -> &'static truvis_descriptor_layout_trait::DescriptorBindingItem {
                    // OnceLock 的开销：get 大约是 1~3 cycles
                    static CURSOR: std::sync::OnceLock<truvis_descriptor_layout_trait::DescriptorBindingItem> = std::sync::OnceLock::new();
                    CURSOR.get_or_init(|| truvis_descriptor_layout_trait::DescriptorBindingItem{
                        name: stringify!(#field_names).trim_matches('_'),
                        binding: #binding_values,
                        descriptor_type: #descriptor_types,
                        stage_flags: #stages,
                        count: #counts,
                        flags: #flags,
                    })
                }
            )*
        }

        impl truvis_descriptor_layout_trait::DescriptorBindingLayout for #struct_name {
            fn get_shader_bindings() -> Vec<truvis_descriptor_layout_trait::DescriptorBindingItem> {
                vec![
                    #(truvis_descriptor_layout_trait::DescriptorBindingItem {
                        name: stringify!(#field_names).trim_matches('_'),
                        binding: #binding_values,
                        descriptor_type: #descriptor_types,
                        stage_flags: #stages,
                        count: #counts,
                        flags: #flags,
                    }),*
                ]
            }
        }
    };

    expanded.into()
}

/// 从字段属性中获取 binding 值
///
/// 属性格式示例：#[binding = 0]
fn get_binding_value(attrs: &[Attribute]) -> Option<u32> {
    for attr in attrs {
        if attr.path().is_ident("binding")
            && let Meta::NameValue(meta) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(lit_int),
                ..
            }) = &meta.value
        {
            return Some(lit_int.base10_parse().unwrap());
        }
    }
    None
}

/// 从字段属性中获取 descriptor_type 值
///
/// 属性格式示例：#[descriptor_type = "UNIFORM_BUFFER"]
fn get_descriptor_type(attrs: &[Attribute]) -> syn::Expr {
    for attr in attrs {
        if attr.path().is_ident("descriptor_type")
            && let Meta::NameValue(meta) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &meta.value
        {
            let descriptor_type = format!("vk::DescriptorType::{}", lit_str.value().as_str());
            return syn::parse_str(&descriptor_type).unwrap();
        }
    }
    // 默认值：统一缓冲区
    syn::parse_quote!(vk::DescriptorType::UNIFORM_BUFFER)
}

/// 从字段属性中获取 count 值
///
/// 属性格式示例：#[count = 1]
fn get_count_value(attrs: &[Attribute]) -> syn::Expr {
    for attr in attrs {
        if attr.path().is_ident("count")
            && let Meta::NameValue(meta) = &attr.meta
        {
            return meta.value.clone();
        }
    }
    // 默认值：1
    syn::parse_quote!(1)
}

/// 从字段属性中获取 stage 值
///
/// 属性格式示例：#[stage = "VERTEX | FRAGMENT"]
fn get_stage_value(attrs: &[Attribute]) -> syn::Expr {
    for attr in attrs {
        if attr.path().is_ident("stage")
            && let Meta::NameValue(meta) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &meta.value
        {
            let stage = lit_str.value();
            let stage_flags =
                stage.split(" | ").map(|s| format!("vk::ShaderStageFlags::{}", s)).collect::<Vec<_>>().join(" | ");
            return syn::parse_str(&stage_flags).unwrap();
        }
    }

    // 默认值：顶点和片段着色器
    syn::parse_quote!(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
}

/// 从字段属性中获取 flags 值
///
/// 属性格式示例：#[flags = "UPDATE_AFTER_BIND | PARTIALLY_BOUND"]
fn get_flags_value(attrs: &[Attribute]) -> syn::Expr {
    for attr in attrs {
        if attr.path().is_ident("flags")
            && let Meta::NameValue(meta) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &meta.value
        {
            let flags = lit_str.value();
            let binding_flags = flags
                .split(" | ")
                .map(|s| format!("vk::DescriptorBindingFlags::{}", s))
                .collect::<Vec<_>>()
                .join(" | ");
            return syn::parse_str(&binding_flags).unwrap();
        }
    }

    // 默认值：空标志
    syn::parse_quote!(vk::DescriptorBindingFlags::empty())
}
