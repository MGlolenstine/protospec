use super::*;
use crate::coder::decode::*;
use crate::{coder::*, map_async};

fn emit_target(target: &Target) -> TokenStream {
    match target {
        Target::Direct => quote! { reader },
        Target::Stream(x) => emit_register(*x),
        Target::Buf(x) => {
            let buf = emit_register(*x);
            quote! { (&mut #buf) }
        }
    }
}

fn prepare_decode(
    context: &Context,
    instructions: &[Instruction],
    is_async: bool,
    is_root: bool,
) -> TokenStream {
    let async_ = map_async(is_async);
    let mut statements = vec![];
    if is_root {
        if is_async {
            statements.push(quote! {
                use tokio::io::{ AsyncRead, AsyncBufRead, AsyncBufReadExt, AsyncReadExt };
            })
        } else {
            statements.push(quote! {
                use std::io::Read;
            })
        }
    }

    for instruction in instructions.iter() {
        match instruction {
            Instruction::Eval(target, expr) => {
                let target = emit_register(*target);
                let value = emit_expression(expr, &|field| {
                    emit_register(
                        *context
                            .field_register_map
                            .get(&field.name)
                            .expect("missing register for field"),
                    )
                });
                statements.push(quote! {
                    let #target = #value;
                });
            }
            Instruction::Construct(target, Constructable::Tuple(items)) => {
                let target = emit_register(*target);
                let items = flatten(
                    items
                        .iter()
                        .map(|x| {
                            let x = emit_register(*x);
                            quote! {#x, }
                        })
                        .collect::<Vec<_>>(),
                );
                statements.push(quote! {
                    let #target = (#items);
                });
            }
            Instruction::Construct(target, Constructable::TaggedTuple { name, items }) => {
                let target = emit_register(*target);
                let items = flatten(
                    items
                        .iter()
                        .map(|x| {
                            let x = emit_register(*x);
                            quote! {#x, }
                        })
                        .collect::<Vec<_>>(),
                );
                let name = emit_ident(name);
                statements.push(quote! {
                    let #target = #name(#items);
                });
            }
            Instruction::Construct(target, Constructable::Struct { name, items }) => {
                let target = emit_register(*target);
                let items = flatten(
                    items
                        .iter()
                        .map(|(name, x)| {
                            let x = emit_register(*x);
                            let name = emit_ident(name);
                            quote! {#name: #x,}
                        })
                        .collect::<Vec<_>>(),
                );
                let name = emit_ident(name);
                statements.push(quote! {
                    let #target = #name  { #items };
                });
            }
            Instruction::Constrict(stream, new_stream, len) => {
                let stream = emit_target(stream);
                let new_stream = emit_register(*new_stream);
                let len = emit_register(*len);
                statements.push(quote! {
                    let mut #new_stream = #stream.take(#len as u64);
                    let #new_stream = &mut #new_stream;
                });
            }
            Instruction::WrapStream(stream, new_stream, transformer, args) => {
                let new_stream_value = emit_register(*new_stream);
                let args = args.iter().map(|x| emit_register(*x)).collect::<Vec<_>>();
                let input = emit_target(stream);
                let transformed = transformer.inner.decoding_gen(input, args, is_async);
                statements.push(quote! {
                    let mut #new_stream_value = #transformed;
                    let #new_stream_value = &mut #new_stream_value;
                })
            }
            Instruction::ConditionalWrapStream(
                condition,
                prelude,
                stream,
                new_stream,
                transformer,
                args,
            ) => {
                let condition = emit_register(*condition);
                let new_stream_value = emit_register(*new_stream);
                let args = args.iter().map(|x| emit_register(*x)).collect::<Vec<_>>();
                let input = emit_target(stream);
                let transformed = transformer
                    .inner
                    .decoding_gen(input.clone(), args, is_async);
                let prelude = prepare_decode(context, &prelude[..], is_async, false);

                //todo: would be nicer to use generics here instead of trait object
                if is_async {
                    statements.push(quote! {
                        let mut r_xform;
                        let #new_stream_value: &mut dyn AsyncBufRead + Unpin + Send + Sync = if #condition {
                            #prelude
                            r_xform = #transformed;
                            &mut r_xform
                        } else {
                            #input as &mut dyn AsyncBufRead + Unpin + Send + Sync
                        };
                    })
                } else {
                    statements.push(quote! {
                        let mut r_xform;
                        let #new_stream_value: &mut dyn Read = if #condition {
                            #prelude
                            r_xform = #transformed;
                            &mut r_xform
                        } else {
                            #input as &mut dyn Read
                        };
                    })
                }
            }
            Instruction::DecodeForeign(target, data, type_ref, args) => {
                let target = emit_target(target);
                let data = emit_register(*data);
                let mut out_arguments = vec![];
                for argument in args {
                    let value = emit_register(*argument);
                    out_arguments.push(value);
                }

                statements.push(
                    type_ref
                        .obj
                        .decoding_gen(target, data, out_arguments, is_async),
                );
            }
            Instruction::DecodeRef(target, source, class, args) => {
                let mut out_arguments = vec![];
                for argument in args {
                    let value = emit_register(*argument);
                    out_arguments.push(quote! {, #value});
                }
                let out_arguments = flatten(out_arguments);
                let target = emit_target(target);
                let source = emit_register(*source);
                let class = emit_ident(class);
                if is_async {
                    statements.push(quote! {
                        let #source = #class::decode_async(#target #out_arguments).await?;
                    });
                } else {
                    statements.push(quote! {
                        let #source = #class::decode_sync(#target #out_arguments)?;
                    });
                }
            }
            Instruction::DecodeEnum(name, type_, value, target) => {
                let target = emit_target(target);
                let value = emit_register(*value);

                let enum_ident = format_ident!("{}", &name);
                let rep = format_ident!("{}", type_.to_string());
                let length = type_.size() as usize;

                statements.push(quote! {
                    let #value = {
                        let mut scratch = [0u8; #length];
                        #target.read_exact(&mut scratch[..])#async_?;
                        #enum_ident::from_repr(#rep::from_be_bytes((&scratch[..]).try_into()?))?
                    };
                });
            }
            Instruction::DecodePrimitive(target, data, PrimitiveType::Bool) => {
                let target = emit_target(target);
                let data = emit_register(*data);

                statements.push(quote! {
                    let #data = {
                        let mut scratch = [0u8; 1];
                        #target.read_exact(&mut scratch[..1])#async_?;
                        scratch[0] != 0
                    };
                });
            }
            Instruction::DecodePrimitive(target, data, type_) => {
                let target = emit_target(target);
                let data = emit_register(*data);
                let length = type_.size() as usize;

                statements.push(quote! {
                    let #data = {
                        let mut scratch = [0u8; #length];
                        #target.read_exact(&mut scratch[..])#async_?;
                        #type_::from_be_bytes((&scratch[..]).try_into()?)
                    };
                });
            }
            Instruction::DecodePrimitiveArray(target, data, type_, len) => {
                let target = emit_target(target);
                let data = emit_register(*data);
                if let Some(len) = len {
                    let len = emit_register(*len);
                    statements.push(quote! {
                        let #data = {
                            let t_count = #len as usize;
                            let mut t: Vec<#type_> = Vec::with_capacity(t_count);
                            unsafe { t.set_len(t_count); }
                            let t_borrow = &mut t[..];
                            let t_borrow2 = unsafe {
                                let len = t_borrow.len() * mem::size_of::<#type_>();
                                let ptr = t.as_ptr() as *mut u8;
                                slice::from_raw_parts_mut(ptr, len)
                            };
                            #target.read_exact(&mut t_borrow2[..])#async_?;
                            t
                        };
                    });
                } else {
                    statements.push(quote! {
                        let #data = {
                            let mut t: Vec<u8> = Vec::new();
                            #target.read_to_end(&mut t)#async_?;
                            let t = Box::leak(t.into_boxed_slice());
                            let size = t.len() / mem::size_of::<#type_>();
                            unsafe { Vec::<#type_>::from_raw_parts(t.as_mut_ptr() as *mut #type_, size, size) }
                        };
                    });
                }
            }
            Instruction::Loop(target, stop_index, terminator, output, inner) => {
                let output = emit_register(*output);
                let inner = prepare_decode(context, &inner[..], is_async, false);
                let stop = stop_index.map(emit_register);
                let terminator = terminator.map(emit_register);
                let target = emit_target(target);
                if let Some(stop) = stop {
                    statements.push(quote! {
                        let mut #output = Vec::with_capacity(#stop as usize);
                        for _ in 0..#stop {
                            #inner
                        }
                    });
                } else if let Some(terminator) = terminator {
                    statements.push(quote! {
                        let mut #output = Vec::new();
                        loop {
                            let buf = #target.fill_buf()#async_?;
                            if buf.len() == 0 {
                                break;
                            }
                            if (buf.len() < #terminator.len()) {
                                //todo: confirm this cannot infinite loop
                                continue;
                            }
                            if &buf[..#terminator.len()] == #terminator {
                                #target.consume(#terminator.len());
                                break;
                            }
                            #inner
                        }
                    });
                } else {
                    statements.push(quote! {
                        let mut #output = Vec::new();
                        //TODO: optimize this to not buffer with a Peekable type
                        {
                            let mut r = vec![];
                            #target.read_to_end(&mut r)#async_?;
                            let r_len = r.len() as u64;

                            {
                                let mut #target = Cursor::new(r);
                                let #target = &mut reader;
                                while reader.position() < r_len {
                                    #inner
                                }
                            }
                        }
                    });
                }
            }
            Instruction::LoopOutput(output, item) => {
                let output = emit_register(*output);
                let item = emit_register(*item);
                statements.push(quote! {
                    #output.push(#item);
                });
            }
            Instruction::Conditional(target, interior, condition, inner) => {
                let target = emit_register(*target);
                let interior = emit_register(*interior);
                let condition = emit_register(*condition);
                let inner = prepare_decode(context, &inner[..], is_async, false);
                statements.push(quote! {
                    let #target = if #condition {
                        #inner
                        Some(#interior)
                    } else {
                        None
                    };
                });
            }
        }
    }

    let statements = flatten(statements);
    quote! {
        #statements
    }
}

pub fn prepare_decoder(coder: &Context, is_async: bool) -> TokenStream {
    let decode = prepare_decode(&coder, &coder.instructions[..], is_async, true);
    let out_reg = emit_register(coder.register_count - 1);
    quote! {
        #decode
        Ok(#out_reg)
    }
}