#[macro_use]
extern crate nuklear_rust;
extern crate nuklear_backend_gfx;

extern crate image;

extern crate gfx;
extern crate gfx_window_glutin;
extern crate glutin;

use nuklear_rust::*;
use nuklear_backend_gfx::{Drawer, GfxBackend};

use glutin::{GlContext, GlRequest};
use gfx::Device as Gd;

use std::fs::*;
use std::io::BufReader;

pub type ColorFormat = gfx::format::Rgba8;
pub type DepthFormat = gfx::format::DepthStencil;

const MAX_VERTEX_MEMORY: usize = 512 * 1024;
const MAX_ELEMENT_MEMORY: usize = 128 * 1024;
const MAX_COMMANDS_MEMORY: usize = 64 * 1024;

struct WeaverState {
    text: NkTextEdit,
    commands: Vec<String>,
}

#[allow(dead_code)]
struct Media {
    font_14: Box<NkFont>,
    font_18: Box<NkFont>,
    font_20: Box<NkFont>,
    font_22: Box<NkFont>,

    font_tex: NkHandle,

    icon_servers: NkImage,
}

fn icon_load<F, R: gfx::Resources>(factory: &mut F, drawer: &mut Drawer<R>, filename: &str) -> NkImage
    where F: gfx::Factory<R>
{

    let img = image::load(BufReader::new(File::open(filename).unwrap()), image::PNG)
        .unwrap()
        .to_rgba();

    let (w, h) = img.dimensions();
    let mut hnd = drawer.add_texture(factory, &img, w, h);

    NkImage::with_id(hnd.id().unwrap())
}

fn main() {
    let gl_version = GlRequest::GlThenGles {
        opengles_version: (2, 0),
        opengl_version: (3, 3),
    };

    let builder = glutin::WindowBuilder::new()
	    .with_title("Weaver")
        .with_dimensions(1280, 800);

    let context = glutin::ContextBuilder::new()
	    .with_gl(gl_version)
	    .with_vsync(true)
	    .with_srgb(false)
	    .with_depth_buffer(24);
    let mut event_loop = glutin::EventsLoop::new();
    let (window, mut device, mut factory, main_color, mut main_depth) = gfx_window_glutin::init::<ColorFormat, DepthFormat>(builder, context, &event_loop);
    let mut encoder: gfx::Encoder<_, _> = factory.create_command_buffer().into();

    let mut cfg = NkFontConfig::with_size(0.0);
    cfg.set_oversample_h(3);
    cfg.set_oversample_v(2);
    cfg.set_glyph_range(nuklear_rust::font_cyrillic_glyph_ranges());
    cfg.set_ttf(include_bytes!("../../res/fonts/Inconsolata-Regular.ttf"));
    //cfg.set_ttf_data_owned_by_atlas(true);

    let mut allo = NkAllocator::new_vec();

    let mut drawer = Drawer::new(&mut factory,
                                 main_color,
                                 36,
                                 MAX_VERTEX_MEMORY,
                                 MAX_ELEMENT_MEMORY,
                                 NkBuffer::with_size(&mut allo, MAX_COMMANDS_MEMORY),
                                 GfxBackend::OpenGlsl150);

    let mut atlas = NkFontAtlas::new(&mut allo);

    cfg.set_size(14f32);
    let font_14 = atlas.add_font_with_config(&cfg).unwrap();
    cfg.set_size(18f32);
    let font_18 = atlas.add_font_with_config(&cfg).unwrap();
    cfg.set_size(20f32);
    let font_20 = atlas.add_font_with_config(&cfg).unwrap();
    cfg.set_size(22f32);
    let font_22 = atlas.add_font_with_config(&cfg).unwrap();

    let font_tex = {
        let (b, w, h) = atlas.bake(NkFontAtlasFormat::NK_FONT_ATLAS_RGBA32);
        drawer.add_texture(&mut factory, b, w, h)
    };

    let mut null = NkDrawNullTexture::default();

    atlas.end(font_tex, Some(&mut null));

    let mut ctx = NkContext::new(&mut allo, &font_14.handle());

    let mut media = Media {
        font_14: font_14,
        font_18: font_18,
        font_20: font_20,
        font_22: font_22,

        font_tex: font_tex,

        icon_servers: icon_load(&mut factory, &mut drawer, "res/icons/servers.png"),
    };

    let mut input_text : NkTextEdit = NkTextEdit::default();
    input_text.init(&mut allo, 4096);

    let mut weaver_state = WeaverState {
        text: input_text,
        commands: Vec::new(),
    };


    let mut mx = 0;
    let mut my = 0;

    let mut config = NkConvertConfig::default();
    config.set_null(null.clone());
    config.set_circle_segment_count(22);
    config.set_curve_segment_count(22);
    config.set_arc_segment_count(22);
    config.set_global_alpha(1.0f32);
    config.set_shape_aa(NkAntiAliasing::NK_ANTI_ALIASING_ON);
    config.set_line_aa(NkAntiAliasing::NK_ANTI_ALIASING_ON);

    let mut closed = false;
    while !closed {
        ctx.input_begin();
        event_loop.poll_events(|event| {
            if let glutin::Event::WindowEvent { event, .. } = event {
	            match event {
	                glutin::WindowEvent::Closed => closed = true,
	                glutin::WindowEvent::ReceivedCharacter(c) => {
                        //println!("Got Character: {}", c);
	                    ctx.input_unicode(c);
	                },
	                glutin::WindowEvent::KeyboardInput{input: glutin::KeyboardInput {
		                    state,
		                    virtual_keycode,
		                    ..
		                },
	               ..} => {
	                    if let Some(k) = virtual_keycode {
	                        let key = match k {
	                            glutin::VirtualKeyCode::Back => NkKey::NK_KEY_BACKSPACE,
	                            glutin::VirtualKeyCode::Delete => NkKey::NK_KEY_DEL,
	                            glutin::VirtualKeyCode::Up => NkKey::NK_KEY_UP,
	                            glutin::VirtualKeyCode::Down => NkKey::NK_KEY_DOWN,
	                            glutin::VirtualKeyCode::Left => NkKey::NK_KEY_LEFT,
	                            glutin::VirtualKeyCode::Right => NkKey::NK_KEY_RIGHT,
                                glutin::VirtualKeyCode::Return => {
                                    if state == glutin::ElementState::Pressed {
                                        println!("Got Return");
                                        let mut msg: String = String::with_capacity(4096);
                                        unsafe {
                                            let blah : &mut nk_text_edit = ::std::mem::transmute(&mut weaver_state.text);
                                            let crap : *mut nk_str = &mut blah.string;
                                            let rfstr : &[u8] = ::std::slice::from_raw_parts(nk_str_get(crap) as *const u8, (*crap).len as usize);
                                            let wub : &str = ::std::str::from_utf8(rfstr).unwrap();
                                            println!("Msg: {:?}", wub);
                                            msg.push_str(wub);
                                        };
                                        //println!("Got text: {}", msg);
                                        //weaver_state.text.delete(0,4096);
                                        weaver_state.text.select_all();
                                        weaver_state.text.delete_selection();
                                        weaver_state.commands.push(msg);
                                        println!("Commands: {}", weaver_state.commands.len());
                                    }
                                    NkKey::NK_KEY_NONE
                                }
	                            _ => NkKey::NK_KEY_NONE,
	                        };
	
	                        ctx.input_key(key, state == glutin::ElementState::Pressed);
	                    }
	                },
	                glutin::WindowEvent::CursorMoved{position: (x, y), ..} => {
	                    mx = x as i32;
	                    my = y as i32;
	                    ctx.input_motion(x as i32, y as i32);
	                },
	                glutin::WindowEvent::MouseInput{state, button, ..} => {
	                    let button = match button {
	                        glutin::MouseButton::Left => NkButton::NK_BUTTON_LEFT,
	                        glutin::MouseButton::Middle => NkButton::NK_BUTTON_MIDDLE,
	                        glutin::MouseButton::Right => NkButton::NK_BUTTON_RIGHT,
	                        _ => NkButton::NK_BUTTON_MAX,
	                    };
	
	                    ctx.input_button(button, mx, my, state == glutin::ElementState::Pressed)
	                },
	                glutin::WindowEvent::MouseWheel{delta, ..} => {
	                    if let glutin::MouseScrollDelta::LineDelta(_, y) = delta {
	                        ctx.input_scroll(y * 22f32);
	                    }
	                },
	                glutin::WindowEvent::Resized(_, _) => {
	                    let mut main_color = drawer.col.clone().unwrap();
	                    gfx_window_glutin::update_views(&window, &mut main_color, &mut main_depth);
	                    drawer.col = Some(main_color);
	                },
	                _ => (),
	            }
            }
        });
        ctx.input_end();
        
        if closed { break; }

        //println!("{:?}", event);
        let (fw, fh) = window.get_inner_size().unwrap();
        let scale = window.hidpi_factor();
        let scale = NkVec2 {
            x: scale,
            y: scale,
        };

        render_weaver(&mut ctx, &mut media, &mut weaver_state, fw, fh);

        encoder.clear(drawer.col.as_ref().unwrap(),
                      [0.1f32, 0.2f32, 0.3f32, 1.0f32]);
        drawer.draw(&mut ctx,
                    &mut config,
                    &mut encoder,
                    &mut factory,
                    fw,
                    fh,
                    scale);
        encoder.flush(&mut device);
        window.swap_buffers().unwrap();
        device.cleanup();

        ::std::thread::sleep(::std::time::Duration::from_millis(20));

        ctx.clear();
    }
    
    // TODO as we do not own the memory of `NkFont`'s, we cannot allow bck to drop it. 
    // Need to find another non-owned wrapper for them, instead of Box.
    ::std::mem::forget(media); 

    atlas.clear();
    ctx.free();
}

use nuklear_rust::nuklear_sys::{nk_text_edit, nk_rune, nk_str_get};
unsafe extern "C" fn printable_filter(_editor: *const nk_text_edit, c: nk_rune) -> ::std::os::raw::c_int {
    if c >= 0x20 {
        return 1;
    } else {
        return 0;
    }
}

use nuklear_rust::nuklear_sys::{nk_edit_focus, nk_edit_flags, nk_flags, nk_context, nk_str};
fn render_weaver(ctx: &mut NkContext, media: &mut Media, state: &mut WeaverState, w_width: u32, w_height: u32) {
    let input_height = 44f32;
    if ctx.begin(nk_string!("Input"),
                 NkRect {
                     x: 0f32,
                     y: (w_height as f32) -input_height,
                     w: (w_width as f32),
                     h: input_height,
                 },
                 NkPanelFlags::NK_WINDOW_BORDER as NkFlags | NkPanelFlags::NK_WINDOW_NO_SCROLLBAR as NkFlags) {
        ctx.style_set_font(&media.font_18.handle());
        ctx.layout_row_dynamic(30f32, 1);

        // This doesn't work D: D:
        unsafe {
            let blah : &*mut nk_context = ::std::mem::transmute(&ctx);
            nk_edit_focus(*blah, nk_edit_flags::NK_EDIT_ALWAYS_INSERT_MODE as nk_flags);
        }

        ctx.edit_buffer(
            NkEditType::NK_EDIT_FIELD as NkFlags,
            &mut state.text,
            Some(printable_filter),
        );
    }
    ctx.end();
    ctx.window_set_focus(nk_string!("Input"));
    if ctx.begin(nk_string!("History"),
                 NkRect {
                     x: 0f32,
                     y: 0f32,
                     w: (w_width as f32),
                     h: (w_height as f32) - input_height,
                 },
                 NkPanelFlags::NK_WINDOW_BORDER as NkFlags | NkPanelFlags::NK_WINDOW_SCROLL_AUTO_HIDE as NkFlags) {
        ctx.style_set_font(&media.font_18.handle());
        ctx.layout_row_dynamic(30f32, 1);

        ctx.text("Test 1", NkTextAlignment::NK_TEXT_LEFT as NkFlags);
        ctx.text("Test 2", NkTextAlignment::NK_TEXT_LEFT as NkFlags);
        for line in &state.commands {
            ctx.text(line, NkTextAlignment::NK_TEXT_LEFT as NkFlags);
        }
    }
    ctx.end();
}