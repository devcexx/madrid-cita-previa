use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    str::FromStr,
};

use lazy_static::lazy_static;
use madrid_cita_previa::{
    DataGenModel, DataGenOffice, DataGenOfficeProcedure, DataGenProcedure, OfficeId, ProcedureId,
    ProcedureOfficeId,
};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::{TokenStreamExt, quote};
use regex::Regex;

lazy_static! {
    static ref RE_DENIED_IDENT_CHARS: Regex = Regex::new("[^0-9A-Za-z_áéíóúñüçÁÉÍÓÚÑÜÇ]").unwrap();
}

fn gen_office_id(office_id: OfficeId) -> TokenStream {
    let lit = Literal::u32_unsuffixed(office_id.0);
    quote! {
        ::madrid_cita_previa::OfficeId(#lit)
    }
}

fn gen_proc_office_id(proc_office_id: ProcedureOfficeId) -> TokenStream {
    let lit = Literal::u32_unsuffixed(proc_office_id.0);
    quote! {
        ::madrid_cita_previa::ProcedureOfficeId(#lit)
    }
}

fn gen_proc_id(proc_id: ProcedureId) -> TokenStream {
    let lit = Literal::u32_unsuffixed(proc_id.0);
    quote! {
        ::madrid_cita_previa::ProcedureId(#lit)
    }
}

fn clean_ident_name(source: &str) -> String {
    let mut clean: String = RE_DENIED_IDENT_CHARS.replace_all(source, "_").to_string();
    while clean.contains("__") {
        clean = clean.replace("__", "_")
    }
    if clean.starts_with("_") {
        clean.remove(0);
    }

    if clean.ends_with("_") {
        clean.remove(clean.len() - 1);
    }

    clean
}

fn office_const_name(office: &DataGenOffice) -> String {
    format!("OFFICE_{}", clean_ident_name(&office.name.to_uppercase()))
}

fn procedure_const_name(proc: &DataGenProcedure) -> String {
    format!(
        "PROC_{}",
        clean_ident_name(&proc.procedure_name.to_uppercase())
    )
}

fn gen_office_procedure(proc: &DataGenOfficeProcedure) -> TokenStream {
    let category_lit = Literal::string(&proc.procedure_category);
    let name_lit = Literal::string(&proc.procedure_name);
    let proc_office_id = gen_proc_office_id(proc.procedure_office_id);
    let proc_id = gen_proc_id(proc.procedure_id);

    quote! {
        ::madrid_cita_previa::StaticOfficeProcedure {
            procedure_name: #name_lit,
            procedure_category: #category_lit,
            procedure_office_id: #proc_office_id,
            procedure_id: #proc_id,
        }
    }
}

fn gen_office(office: &DataGenOffice) -> TokenStream {
    let office_const = Ident::new(&office_const_name(office), Span::call_site());

    let office_name_lit = Literal::string(&office.name);
    let office_group_lit = Literal::string(&office.group);
    let office_id = gen_office_id(office.id);
    let procedures = office
        .procedures
        .iter()
        .map(|proc| gen_office_procedure(proc));

    quote! {
        pub const #office_const: ::madrid_cita_previa::StaticOffice = ::madrid_cita_previa::StaticOffice {
            name: #office_name_lit,
            group: #office_group_lit,
            id: #office_id,
            procedures: &[
                #(#procedures),*
            ]
        };
    }
}

fn gen_offices_mod(model: &DataGenModel) -> TokenStream {
    let all_offices_refs = model.offices.iter().map(|office| {
        let ident = Ident::new(&office_const_name(office), Span::call_site());
        quote! {
            &#ident
        }
    });

    let all_gen_offices = model.offices.iter().map(|office| gen_office(office));

    quote! {
        pub mod offices {
            #(#all_gen_offices)*
            pub const ALL: &[&::madrid_cita_previa::StaticOffice] = &[
                #(#all_offices_refs),*
            ];
        }
    }
}

fn gen_procedure(proc: &DataGenProcedure) -> TokenStream {
    let ident = Ident::new(&procedure_const_name(proc), Span::call_site());

    let proc_category_lit = Literal::string(&proc.procedure_category);
    let proc_name_lit = Literal::string(&proc.procedure_name);
    let proc_id = gen_proc_id(proc.procedure_id);

    quote! {
        pub const #ident: ::madrid_cita_previa::StaticProcedure = ::madrid_cita_previa::StaticProcedure {
            procedure_category: #proc_category_lit,
            procedure_name: #proc_name_lit,
            procedure_id: #proc_id
        };
    }
}

fn gen_procedures_mod(model: &DataGenModel) -> TokenStream {
    let all_procs_refs = model.procedures.iter().map(|proc| {
        let ident = Ident::new(&procedure_const_name(proc), Span::call_site());
        quote! {
            &#ident
        }
    });

    let all_procs = model.procedures.iter().map(|proc| gen_procedure(proc));

    quote! {
        pub mod procedures {
            #(#all_procs)*
            pub const ALL: &[&::madrid_cita_previa::StaticProcedure] = &[
                #(#all_procs_refs),*
            ];
        }
    }
}

const MODEL_PATH: &str = "../../data/model.json";

fn main() {
    println!("cargo::rerun-if-changed={}", MODEL_PATH);
    let mut model_contents: String = String::new();
    File::open(MODEL_PATH)
        .expect(&format!(
            "Couldn't open model file at {}. Have you ran the datagen script?",
            MODEL_PATH
        ))
        .read_to_string(&mut model_contents)
        .unwrap();
    let datagen_model: DataGenModel = serde_json::from_str(&model_contents).unwrap();

    let mut tokens = TokenStream::new();
    tokens.append_all(gen_offices_mod(&datagen_model));
    tokens.append_all(gen_procedures_mod(&datagen_model));

    let str = tokens.to_string();

    let out_path = PathBuf::from_str(&std::env::var("OUT_DIR").unwrap())
        .unwrap()
        .join("gen.rs");
    File::create(out_path)
        .unwrap()
        .write_all(&str.as_bytes())
        .unwrap();
}
