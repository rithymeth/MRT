//! The built-in library.
//!
//! Every message here is matched word for word against the reference
//! implementation, because programs catch errors and print `e.message`: a
//! reworded message is a behaviour change, not a cosmetic one.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ai::{tensor_from_mrt, tensor_to_mrt};
use crate::env::Env;
use crate::error::{arity_error, type_error, value_error, Eval, Kind, Signal};
use crate::game::{channel, color, coord};
use crate::generator::{step_result, Generator, Transform};
use crate::value::*;
use crate::{is_iterable, object_fields, Interpreter};

const NAMES: &[&str] = &[
    "len",
    "push",
    "pop",
    "slice",
    "join",
    "indexOf",
    "split",
    "substring",
    "toUpper",
    "toLower",
    "trim",
    "replace",
    "startsWith",
    "endsWith",
    "contains",
    "type",
    "toNumber",
    "toString",
    "abs",
    "min",
    "max",
    "round",
    "floor",
    "ceil",
    "sqrt",
    "pow",
    "keys",
    "values",
    "has",
    "get",
    "reverse",
    "unique",
    "flatten",
    "zip",
    "enumerate",
    "count",
    "sum",
    "range",
    "repeat",
    "padStart",
    "padEnd",
    "random",
    "map",
    "filter",
    "reduce",
    "find",
    "some",
    "every",
    "sort",
    "toArray",
    "take",
    "next",
    "send",
    "print",
    "aiTrainLinear",
    "aiDense",
    "aiConv2d",
    "aiAttention",
    "aiLayerNorm",
    "aiActivation",
    "aiPredict",
    "aiTrain",
    "gameInit",
    "gameColor",
    "gameColorAlpha",
    "gameColorParts",
    "gameWidth",
    "gameHeight",
    "gameClear",
    "gamePixel",
    "gameRect",
    "gameRectOutline",
    "gameLine",
    "gameCircle",
    "gameCircleOutline",
    "gameOverlap",
    "gameResolve",
    "gameSweep",
    "gameSurface",
    "gameLoad",
    "gameSurfaceSize",
    "gameTarget",
    "gameTargetId",
    "gameDraw",
    "gameSurfaceFree",
    "gameColorAt",
    "gameText",
    "gameTextWidth",
    "gameTextHeight",
    "gameSave",
    "soundTone",
    "soundSweep",
    "soundLoad",
    "soundSave",
    "soundMix",
    "soundThen",
    "soundGain",
    "soundNormalize",
    "soundLowPass",
    "soundInfo",
    "soundFree",
    "soundPlay",
    "soundStop",
    "soundStopAll",
    "soundPlaying",
    "gameOpen",
    "gamePresent",
    "gameDelta",
    "gameKeyDown",
    "gameKeyPressed",
    "gamePointer",
    "gameClose",
];

pub fn install(globals: &Env) {
    for name in NAMES {
        globals.define(name, Builtin::named(name));
    }
}

pub fn call(interp: &mut Interpreter, name: &str, args: Vec<Value>) -> Eval {
    match name {
        // -- the AI extension's model surface -------------------------------
        // A model is an array of layer objects, so every one of these takes
        // and returns ordinary MRT data. See crate::ai for why that beats an
        // opaque handle.
        "aiDense" => {
            exactly(&args, 3, "aiDense() takes inputs, outputs, and a seed.")?;
            let inputs = whole(&args[0], "aiDense", "inputs")?;
            let outputs = whole(&args[1], "aiDense", "outputs")?;
            let seed = whole(&args[2], "aiDense", "seed")? as u64;
            if inputs == 0 || outputs == 0 {
                return Err(value_error(
                    "aiDense() needs at least one input and output.",
                ));
            }
            Ok(crate::ai::dense(inputs, outputs, seed))
        }
        "aiConv2d" => {
            exactly(
                &args,
                7,
                "aiConv2d() takes channels, height, width, kernel, stride, filters, and a seed.",
            )?;
            crate::ai::conv2d(
                whole(&args[0], "aiConv2d", "channels")?,
                whole(&args[1], "aiConv2d", "height")?,
                whole(&args[2], "aiConv2d", "width")?,
                whole(&args[3], "aiConv2d", "kernel")?,
                whole(&args[4], "aiConv2d", "stride")?,
                whole(&args[5], "aiConv2d", "filters")?,
                whole(&args[6], "aiConv2d", "seed")? as u64,
            )
        }
        "aiAttention" => {
            exactly(&args, 3, "aiAttention() takes features, head, and a seed.")?;
            let features = whole(&args[0], "aiAttention", "features")?;
            let head = whole(&args[1], "aiAttention", "head")?;
            if features == 0 || head == 0 {
                return Err(value_error(
                    "aiAttention() needs at least one feature and one head column.",
                ));
            }
            Ok(crate::ai::attention(
                features,
                head,
                whole(&args[2], "aiAttention", "seed")? as u64,
            ))
        }
        "aiLayerNorm" => {
            one(&args, "aiLayerNorm")?;
            let features = whole(&args[0], "aiLayerNorm", "features")?;
            if features == 0 {
                return Err(value_error("aiLayerNorm() needs at least one feature."));
            }
            Ok(crate::ai::layer_norm(features))
        }
        "aiActivation" => {
            one(&args, "aiActivation")?;
            let Value::Str(kind) = &args[0] else {
                return Err(type_error(format!(
                    "aiActivation() expects a string, not {}.",
                    type_name(&args[0])
                )));
            };
            crate::ai::activation(kind)
        }
        "aiPredict" => {
            exactly(&args, 2, "aiPredict() takes a model and an input.")?;
            crate::ai::predict(&args[0], &args[1])
        }
        "aiTrain" => {
            exactly(
                &args,
                5,
                "aiTrain() takes a model, inputs, targets, epochs, and a learning rate.",
            )?;
            crate::ai::train(
                &args[0],
                &args[1],
                &args[2],
                whole(&args[3], "aiTrain", "epochs")?,
                number(&args[4], "aiTrain")?,
            )
        }

        // -- the game extension's drawing surface ---------------------------
        // The screen lives on the interpreter, not in an MRT value: a
        // framebuffer is too large to be data the way a model is. See
        // crate::game.
        "gameInit" => {
            exactly(&args, 3, "gameInit() takes a width, a height, and a title.")?;
            let width = whole(&args[0], "gameInit", "the width")?;
            let height = whole(&args[1], "gameInit", "the height")?;
            let Value::Str(title) = &args[2] else {
                return Err(type_error(format!(
                    "gameInit() needs the title to be a string, not {}.",
                    type_name(&args[2])
                )));
            };
            interp.screen = Some(crate::game::init(width, height, title)?);
            Ok(Value::Null)
        }
        "gameColor" => {
            exactly(&args, 3, "gameColor() takes red, green, and blue.")?;
            Ok(crate::game::make_color(
                channel(&args[0], "gameColor", "red")?,
                channel(&args[1], "gameColor", "green")?,
                channel(&args[2], "gameColor", "blue")?,
                255,
            ))
        }
        "gameColorAlpha" => {
            exactly(
                &args,
                4,
                "gameColorAlpha() takes red, green, blue, and alpha.",
            )?;
            Ok(crate::game::make_color(
                channel(&args[0], "gameColorAlpha", "red")?,
                channel(&args[1], "gameColorAlpha", "green")?,
                channel(&args[2], "gameColorAlpha", "blue")?,
                channel(&args[3], "gameColorAlpha", "alpha")?,
            ))
        }
        "gameColorParts" => {
            one(&args, "gameColorParts")?;
            Ok(crate::game::split_color(color(&args[0], "gameColorParts")?))
        }
        "gameWidth" => {
            exactly(&args, 0, "gameWidth() takes no arguments.")?;
            Ok(Value::Number(
                screen_of(interp, "gameWidth")?.target().width as f64,
            ))
        }
        "gameHeight" => {
            exactly(&args, 0, "gameHeight() takes no arguments.")?;
            Ok(Value::Number(
                screen_of(interp, "gameHeight")?.target().height as f64,
            ))
        }
        "gameClear" => {
            one(&args, "gameClear")?;
            crate::game::draw::clear(&mut interp.screen, color(&args[0], "gameClear")?)
        }
        "gamePixel" => {
            exactly(&args, 3, "gamePixel() takes x, y, and a colour.")?;
            crate::game::draw::pixel(
                &mut interp.screen,
                coord(&args[0], "gamePixel", "x")?,
                coord(&args[1], "gamePixel", "y")?,
                color(&args[2], "gamePixel")?,
            )
        }
        "gameRect" => {
            exactly(
                &args,
                5,
                "gameRect() takes x, y, a width, a height, and a colour.",
            )?;
            crate::game::draw::rect(
                &mut interp.screen,
                coord(&args[0], "gameRect", "x")?,
                coord(&args[1], "gameRect", "y")?,
                coord(&args[2], "gameRect", "the width")?,
                coord(&args[3], "gameRect", "the height")?,
                color(&args[4], "gameRect")?,
            )
        }
        "gameRectOutline" => {
            exactly(
                &args,
                6,
                "gameRectOutline() takes x, y, a width, a height, a thickness, and a colour.",
            )?;
            crate::game::draw::rect_outline(
                &mut interp.screen,
                coord(&args[0], "gameRectOutline", "x")?,
                coord(&args[1], "gameRectOutline", "y")?,
                coord(&args[2], "gameRectOutline", "the width")?,
                coord(&args[3], "gameRectOutline", "the height")?,
                coord(&args[4], "gameRectOutline", "the thickness")?,
                color(&args[5], "gameRectOutline")?,
            )
        }
        "gameLine" => {
            exactly(&args, 5, "gameLine() takes x1, y1, x2, y2, and a colour.")?;
            crate::game::draw::line(
                &mut interp.screen,
                coord(&args[0], "gameLine", "x1")?,
                coord(&args[1], "gameLine", "y1")?,
                coord(&args[2], "gameLine", "x2")?,
                coord(&args[3], "gameLine", "y2")?,
                color(&args[4], "gameLine")?,
            )
        }
        "gameCircle" => {
            exactly(&args, 4, "gameCircle() takes x, y, a radius, and a colour.")?;
            crate::game::draw::circle(
                &mut interp.screen,
                coord(&args[0], "gameCircle", "x")?,
                coord(&args[1], "gameCircle", "y")?,
                coord(&args[2], "gameCircle", "the radius")?,
                color(&args[3], "gameCircle")?,
            )
        }
        "gameCircleOutline" => {
            exactly(
                &args,
                4,
                "gameCircleOutline() takes x, y, a radius, and a colour.",
            )?;
            crate::game::draw::circle_outline(
                &mut interp.screen,
                coord(&args[0], "gameCircleOutline", "x")?,
                coord(&args[1], "gameCircleOutline", "y")?,
                coord(&args[2], "gameCircleOutline", "the radius")?,
                color(&args[3], "gameCircleOutline")?,
            )
        }
        "gameOverlap" => {
            exactly(&args, 2, "gameOverlap() takes two boxes.")?;
            crate::game::collide::overlap(&args[0], &args[1])
        }
        "gameResolve" => {
            exactly(&args, 2, "gameResolve() takes two boxes.")?;
            crate::game::collide::resolve(&args[0], &args[1])
        }
        "gameSweep" => {
            exactly(
                &args,
                4,
                "gameSweep() takes a moving box, dx, dy, and a box it might hit.",
            )?;
            crate::game::collide::sweep(
                &args[0],
                number(&args[1], "gameSweep")?,
                number(&args[2], "gameSweep")?,
                &args[3],
            )
        }
        "gameSurface" => {
            exactly(&args, 2, "gameSurface() takes a width and a height.")?;
            crate::game::sprites::create(
                &mut interp.screen,
                whole(&args[0], "gameSurface", "the width")?,
                whole(&args[1], "gameSurface", "the height")?,
            )
        }
        "gameLoad" => {
            one(&args, "gameLoad")?;
            let Value::Str(path) = &args[0] else {
                return Err(type_error(format!(
                    "gameLoad() needs a path, not {}.",
                    type_name(&args[0])
                )));
            };
            let path = path.clone();
            crate::game::sprites::load(&mut interp.screen, &path)
        }
        "gameSurfaceSize" => {
            one(&args, "gameSurfaceSize")?;
            crate::game::sprites::size(
                &interp.screen,
                whole(&args[0], "gameSurfaceSize", "the surface")?,
            )
        }
        "gameTarget" => {
            one(&args, "gameTarget")?;
            crate::game::sprites::target(
                &mut interp.screen,
                whole(&args[0], "gameTarget", "the surface")?,
            )
        }
        "gameTargetId" => {
            exactly(&args, 0, "gameTargetId() takes no arguments.")?;
            crate::game::sprites::current_target(&interp.screen)
        }
        "gameDraw" => {
            // The fourth argument is optional so the common case -- no
            // rotation, no scale -- stays the three-argument call every
            // existing program already makes.
            between(
                &args,
                3,
                4,
                "gameDraw() takes a surface, x, y, and optionally {angle, scaleX, scaleY, anchorX, anchorY}.",
            )?;
            let id = whole(&args[0], "gameDraw", "the surface")?;
            let x = coord(&args[1], "gameDraw", "x")?;
            let y = coord(&args[2], "gameDraw", "y")?;
            match args.get(3) {
                None | Some(Value::Null) => {
                    crate::game::sprites::draw(&mut interp.screen, id, x, y)
                }
                Some(options) => {
                    let transform = crate::game::sprites::transform_of(options, "gameDraw")?;
                    crate::game::sprites::draw_transformed(&mut interp.screen, id, x, y, &transform)
                }
            }
        }
        "gameSurfaceFree" => {
            one(&args, "gameSurfaceFree")?;
            crate::game::sprites::free(
                &mut interp.screen,
                whole(&args[0], "gameSurfaceFree", "the surface")?,
            )
        }
        "gameColorAt" => {
            // Reading the surface back is what lets a program check its own
            // drawing, which is otherwise only visible to a person looking at
            // a file.
            exactly(&args, 2, "gameColorAt() takes x and y.")?;
            let x = coord(&args[0], "gameColorAt", "x")?;
            let y = coord(&args[1], "gameColorAt", "y")?;
            Ok(match screen_of(interp, "gameColorAt")?.target().get(x, y) {
                Some(c) => Value::Number(c.0 as f64),
                None => Value::Null,
            })
        }
        "gameText" => {
            exactly(
                &args,
                5,
                "gameText() takes x, y, the text, a scale, and a colour.",
            )?;
            let Value::Str(text) = &args[2] else {
                return Err(type_error(format!(
                    "gameText() needs the text to be a string, not {}.",
                    type_name(&args[2])
                )));
            };
            let text = text.clone();
            crate::game::draw::text(
                &mut interp.screen,
                coord(&args[0], "gameText", "x")?,
                coord(&args[1], "gameText", "y")?,
                &text,
                coord(&args[3], "gameText", "the scale")?,
                color(&args[4], "gameText")?,
            )
        }
        "gameTextWidth" | "gameTextHeight" => {
            // Measuring needs no screen: it is arithmetic on the font, and a
            // program laying out a menu before it opens a window should not
            // have to call gameInit to find out how wide a word is.
            exactly(&args, 2, format!("{name}() takes the text and a scale."))?;
            let Value::Str(text) = &args[0] else {
                return Err(type_error(format!(
                    "{name}() needs the text to be a string, not {}.",
                    type_name(&args[0])
                )));
            };
            let scale = coord(&args[1], name, "the scale")?;
            Ok(Value::Number(if name == "gameTextWidth" {
                mrt_game::Surface::text_width(text, scale) as f64
            } else {
                mrt_game::Surface::text_height(text, scale) as f64
            }))
        }
        "gameSave" => {
            one(&args, "gameSave")?;
            let Value::Str(path) = &args[0] else {
                return Err(type_error(format!(
                    "gameSave() needs a path, not {}.",
                    type_name(&args[0])
                )));
            };
            crate::game::save(&interp.screen, path)
        }

        // -- sound ----------------------------------------------------------
        // Sounds are ids, exactly as sprites are, and for the same reason: a
        // second of stereo is 88,200 samples. See crate::sound.
        "soundTone" | "soundSweep" => {
            let takes = if name == "soundTone" { 4 } else { 5 };
            exactly(
                &args,
                takes,
                if name == "soundTone" {
                    "soundTone() takes a wave, a frequency, a length, and an envelope."
                } else {
                    "soundSweep() takes a wave, two frequencies, a length, and an envelope."
                },
            )?;
            let Value::Str(wave) = &args[0] else {
                return Err(type_error(format!(
                    "{name}() needs the wave to be a string, not {}.",
                    type_name(&args[0])
                )));
            };
            let wave = wave.clone();
            if name == "soundTone" {
                crate::sound::tone(&mut interp.sounds, &wave, &args[1], &args[2], &args[3])
            } else {
                crate::sound::sweep(
                    &mut interp.sounds,
                    &wave,
                    &args[1],
                    &args[2],
                    &args[3],
                    &args[4],
                )
            }
        }
        "soundLoad" => {
            one(&args, "soundLoad")?;
            let Value::Str(path) = &args[0] else {
                return Err(type_error(format!(
                    "soundLoad() needs a path, not {}.",
                    type_name(&args[0])
                )));
            };
            let path = path.clone();
            crate::sound::load(&mut interp.sounds, &path)
        }
        "soundSave" => {
            exactly(&args, 2, "soundSave() takes a sound and a path.")?;
            let id = whole(&args[0], "soundSave", "the sound")?;
            let Value::Str(path) = &args[1] else {
                return Err(type_error(format!(
                    "soundSave() needs a path, not {}.",
                    type_name(&args[1])
                )));
            };
            let path = path.clone();
            crate::sound::save(&interp.sounds, id, &path)
        }
        "soundMix" => {
            one(&args, "soundMix")?;
            crate::sound::mix(&mut interp.sounds, &args[0])
        }
        "soundThen" => {
            exactly(&args, 2, "soundThen() takes two sounds.")?;
            crate::sound::then(
                &mut interp.sounds,
                whole(&args[0], "soundThen", "the first sound")?,
                whole(&args[1], "soundThen", "the second sound")?,
            )
        }
        "soundGain" | "soundNormalize" => {
            exactly(&args, 2, format!("{name}() takes a sound and a level."))?;
            crate::sound::gain(
                &mut interp.sounds,
                whole(&args[0], name, "the sound")?,
                number(&args[1], name)?,
                name == "soundNormalize",
            )
        }
        "soundLowPass" => {
            exactly(
                &args,
                2,
                "soundLowPass() takes a sound and a cutoff in hertz.",
            )?;
            crate::sound::low_pass(
                &mut interp.sounds,
                whole(&args[0], "soundLowPass", "the sound")?,
                number(&args[1], "soundLowPass")?,
            )
        }
        "soundInfo" => {
            one(&args, "soundInfo")?;
            crate::sound::info(&interp.sounds, whole(&args[0], "soundInfo", "the sound")?)
        }
        "soundFree" => {
            one(&args, "soundFree")?;
            interp
                .sounds
                .free(whole(&args[0], "soundFree", "the sound")?)
        }

        "soundPlay" => {
            // The volume and the loop flag both have obvious defaults, and a
            // game firing an effect should not have to write them.
            between(
                &args,
                1,
                2,
                "soundPlay() takes a sound, and optionally {volume, pan, speed, loop}.",
            )?;
            let id = whole(&args[0], "soundPlay", "the sound")?;
            let how = crate::sound::play_options(args.get(1).unwrap_or(&Value::Null), "soundPlay")?;
            crate::sound::live::play(&mut interp.sounds, id, how)
        }
        "soundStop" => {
            one(&args, "soundStop")?;
            let voice = whole(&args[0], "soundStop", "the voice")? as u64;
            crate::sound::live::stop(&interp.sounds, Some(voice))
        }
        "soundStopAll" => {
            exactly(&args, 0, "soundStopAll() takes no arguments.")?;
            crate::sound::live::stop(&interp.sounds, None)
        }
        "soundPlaying" => {
            exactly(&args, 0, "soundPlaying() takes no arguments.")?;
            crate::sound::live::playing(&interp.sounds)
        }

        // -- the window half: a game's main loop lives in MRT source ---------
        // `while (gameOpen()) { ...; gamePresent(); }`. See crate::game::live
        // for why the loop is the program's and not the engine's.
        "gameOpen" => {
            exactly(&args, 0, "gameOpen() takes no arguments.")?;
            crate::game::live::open(&mut interp.screen)
        }
        "gamePresent" => {
            exactly(&args, 0, "gamePresent() takes no arguments.")?;
            crate::game::live::present(&mut interp.screen)
        }
        "gameDelta" => {
            exactly(&args, 0, "gameDelta() takes no arguments.")?;
            crate::game::live::delta(&interp.screen)
        }
        "gameKeyDown" | "gameKeyPressed" => {
            one(&args, name)?;
            let Value::Str(key) = &args[0] else {
                return Err(type_error(format!(
                    "{name}() needs a key name, not {}.",
                    type_name(&args[0])
                )));
            };
            if name == "gameKeyDown" {
                crate::game::live::key_down(&interp.screen, key)
            } else {
                crate::game::live::key_pressed(&interp.screen, key)
            }
        }
        "gamePointer" => {
            exactly(&args, 0, "gamePointer() takes no arguments.")?;
            crate::game::live::pointer(&interp.screen)
        }
        "gameClose" => {
            exactly(&args, 0, "gameClose() takes no arguments.")?;
            crate::game::live::close(&mut interp.screen)
        }

        "aiTrainLinear" => {
            exactly(
                &args,
                4,
                "aiTrainLinear() takes x, y, epochs, and learningRate.",
            )?;
            let x = tensor_from_mrt(&args[0], "aiTrainLinear")?;
            let y = tensor_from_mrt(&args[1], "aiTrainLinear")?;
            let epochs = number(&args[2], "aiTrainLinear")? as usize;
            let lr = number(&args[3], "aiTrainLinear")?;
            if epochs == 0 {
                return Err(value_error(
                    "aiTrainLinear() epochs must be greater than zero.",
                ));
            }
            if lr <= 0.0 {
                return Err(value_error(
                    "aiTrainLinear() learningRate must be greater than zero.",
                ));
            }

            let mut model = mrt_ai::Sequential::new();
            model.add(mrt_ai::Dense::new(x.cols, y.cols, 42));
            let mut trainer = mrt_ai::Trainer::new(epochs, lr);
            let history = trainer
                .train(&mut model, &x, &y)
                .map_err(|e| value_error(e.to_string()))?;
            let prediction = model.forward(&x).map_err(|e| value_error(e.to_string()))?;

            let mut result = ObjMap::new();
            result.insert(
                ObjKey::Str(Rc::from("loss")),
                Value::Number(*history.last().unwrap_or(&0.0)),
            );
            result.insert(
                ObjKey::Str(Rc::from("initialLoss")),
                Value::Number(history[0]),
            );
            result.insert(
                ObjKey::Str(Rc::from("predictions")),
                tensor_to_mrt(&prediction),
            );
            result.insert(
                ObjKey::Str(Rc::from("epochs")),
                Value::Number(epochs as f64),
            );
            Ok(Value::object(result))
        }
        // -- arrays --------------------------------------------------------
        "len" => {
            one(&args, "len")?;
            if let Some(fields) = object_fields(&args[0]) {
                return Ok(Value::Number(fields.len() as f64));
            }
            match &args[0] {
                Value::Array(a) => Ok(Value::Number(a.borrow().len() as f64)),
                Value::Str(s) => Ok(Value::Number(s.chars().count() as f64)),
                _ => Err(type_error(
                    "len() argument must be an array, object, or string.",
                )),
            }
        }
        "push" => {
            exactly(&args, 2, "push() takes exactly two arguments.")?;
            let array = array_arg(&args[0], "push")?;
            array.borrow_mut().push(args[1].clone());
            Ok(args[1].clone())
        }
        "pop" => {
            one(&args, "pop")?;
            let Value::Array(a) = &args[0] else {
                return Err(type_error("pop() argument must be an array."));
            };
            a.borrow_mut()
                .pop()
                .ok_or_else(|| value_error("Cannot pop from empty array."))
        }
        "slice" => {
            between(&args, 2, 3, "slice() takes 2 or 3 arguments.")?;
            let array = array_arg(&args[0], "slice")?;
            let items = array.borrow();
            let (start, end) = span(&args, items.len());
            Ok(Value::array(items[start..end].to_vec()))
        }
        "join" => {
            between(&args, 1, 2, "join() takes 1 or 2 arguments.")?;
            let array = array_arg(&args[0], "join")?;
            let sep = args.get(1).map(stringify).unwrap_or_default();
            let parts: Vec<String> = array.borrow().iter().map(stringify).collect();
            Ok(Value::str(parts.join(&sep)))
        }
        "indexOf" => {
            exactly(&args, 2, "indexOf() takes exactly 2 arguments.")?;
            let array = array_arg(&args[0], "indexOf")?;
            let found = array
                .borrow()
                .iter()
                .position(|item| values_equal(item, &args[1]));
            Ok(Value::Number(found.map(|i| i as f64).unwrap_or(-1.0)))
        }
        "reverse" => {
            one(&args, "reverse")?;
            match &args[0] {
                Value::Array(a) => {
                    let mut items = a.borrow().clone();
                    items.reverse();
                    Ok(Value::array(items))
                }
                Value::Str(s) => Ok(Value::str(s.chars().rev().collect::<String>())),
                _ => Err(type_error("reverse() argument must be an array or string.")),
            }
        }
        "unique" => {
            one(&args, "unique")?;
            let array = array_arg(&args[0], "unique")?;
            let mut out: Vec<Value> = Vec::new();
            for item in array.borrow().iter() {
                if !out.iter().any(|seen| values_equal(seen, item)) {
                    out.push(item.clone());
                }
            }
            Ok(Value::array(out))
        }
        "flatten" => {
            between(&args, 1, 2, "flatten() takes 1 or 2 arguments.")?;
            let array = array_arg(&args[0], "flatten")?;
            let depth = match args.get(1) {
                Some(v) => number(v, "flatten")? as i64,
                None => 1,
            };
            let mut out = Vec::new();
            flatten_into(&array.borrow(), depth, &mut out);
            Ok(Value::array(out))
        }
        "zip" => {
            exactly(&args, 2, "zip() takes exactly 2 arguments.")?;
            let a = array_arg(&args[0], "zip")?;
            let b = array_arg(&args[1], "zip")?;
            let (a, b) = (a.borrow(), b.borrow());
            Ok(Value::array(
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| Value::array(vec![x.clone(), y.clone()]))
                    .collect(),
            ))
        }
        "enumerate" => {
            one(&args, "enumerate")?;
            let array = array_arg(&args[0], "enumerate")?;
            let pairs: Vec<Value> = array
                .borrow()
                .iter()
                .enumerate()
                .map(|(i, v)| Value::array(vec![Value::Number(i as f64), v.clone()]))
                .collect();
            Ok(Value::array(pairs))
        }
        "count" => {
            exactly(&args, 2, "count() takes exactly 2 arguments.")?;
            let array = array_arg(&args[0], "count")?;
            let n = array
                .borrow()
                .iter()
                .filter(|item| values_equal(item, &args[1]))
                .count();
            Ok(Value::Number(n as f64))
        }
        "sum" => {
            one(&args, "sum")?;
            let array = array_arg(&args[0], "sum")?;
            let mut total = 0.0;
            for item in array.borrow().iter() {
                total += number(item, "sum")?;
            }
            Ok(Value::Number(total))
        }
        "range" => {
            between(&args, 1, 3, "range() takes one to three number arguments.")?;
            let nums: Result<Vec<f64>, Signal> = args.iter().map(|a| number(a, "range")).collect();
            let nums = nums?;
            let (start, end, step) = match nums.len() {
                1 => (0.0, nums[0], 1.0),
                2 => (nums[0], nums[1], 1.0),
                _ => (nums[0], nums[1], nums[2]),
            };
            if step == 0.0 {
                return Err(value_error("range() step must not be zero."));
            }
            let mut out = Vec::new();
            let mut current = start;
            while (step > 0.0 && current < end) || (step < 0.0 && current > end) {
                out.push(Value::Number(current));
                current += step;
            }
            Ok(Value::array(out))
        }

        // -- strings -------------------------------------------------------
        "split" => {
            between(&args, 1, 2, "split() takes 1 or 2 arguments.")?;
            let text = string_arg(&args[0], "split")?;
            let sep = args.get(1).map(stringify).unwrap_or_else(|| " ".into());
            Ok(Value::array(
                text.split(&sep).map(Value::str).collect::<Vec<_>>(),
            ))
        }
        "substring" => {
            between(&args, 2, 3, "substring() takes 2 or 3 arguments.")?;
            let text = string_arg(&args[0], "substring")?;
            let chars: Vec<char> = text.chars().collect();
            let (start, end) = span(&args, chars.len());
            Ok(Value::str(chars[start..end].iter().collect::<String>()))
        }
        "toUpper" => {
            one(&args, "toUpper")?;
            Ok(Value::str(string_arg1(&args[0], "toUpper")?.to_uppercase()))
        }
        "toLower" => {
            one(&args, "toLower")?;
            Ok(Value::str(string_arg1(&args[0], "toLower")?.to_lowercase()))
        }
        "trim" => {
            one(&args, "trim")?;
            Ok(Value::str(
                string_arg1(&args[0], "trim")?.trim().to_string(),
            ))
        }
        "replace" => {
            exactly(&args, 3, "replace() takes exactly 3 arguments.")?;
            let text = string_arg(&args[0], "replace")?;
            Ok(Value::str(
                text.replace(&stringify(&args[1]), &stringify(&args[2])),
            ))
        }
        "startsWith" => {
            exactly(&args, 2, "startsWith() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "startsWith")?;
            Ok(Value::Bool(text.starts_with(&stringify(&args[1]))))
        }
        "endsWith" => {
            exactly(&args, 2, "endsWith() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "endsWith")?;
            Ok(Value::Bool(text.ends_with(&stringify(&args[1]))))
        }
        "contains" => {
            exactly(&args, 2, "contains() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "contains")?;
            Ok(Value::Bool(text.contains(&stringify(&args[1]))))
        }
        "repeat" => {
            exactly(&args, 2, "repeat() takes exactly 2 arguments.")?;
            let text = string_arg(&args[0], "repeat")?;
            let n = number(&args[1], "repeat")?;
            if n < 0.0 {
                return Err(value_error("repeat() count must not be negative."));
            }
            Ok(Value::str(text.repeat(n as usize)))
        }
        "padStart" | "padEnd" => {
            between(&args, 2, 3, format!("{name}() takes 2 or 3 arguments."))?;
            let text = string_arg(&args[0], name)?;
            let width = number(&args[1], name)? as usize;
            let pad = match args.get(2) {
                Some(v) => stringify(v),
                None => " ".into(),
            };
            let current = text.chars().count();
            if current >= width || pad.is_empty() {
                return Ok(Value::str(text));
            }
            let needed = width - current;
            let filler: String = pad.chars().cycle().take(needed).collect();
            Ok(Value::str(if name == "padStart" {
                format!("{filler}{text}")
            } else {
                format!("{text}{filler}")
            }))
        }

        // -- types and maths -----------------------------------------------
        "type" => {
            one(&args, "type")?;
            Ok(Value::str(type_name(&args[0])))
        }
        "toString" => {
            one(&args, "toString")?;
            Ok(Value::str(stringify(&args[0])))
        }
        "toNumber" => {
            one(&args, "toNumber")?;
            match &args[0] {
                Value::Bool(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
                Value::Number(n) => Ok(Value::Number(*n)),
                Value::Str(s) => s
                    .trim()
                    .parse::<f64>()
                    .map(Value::Number)
                    .map_err(|_| value_error(format!("Cannot convert '{s}' to a number."))),
                _ => Err(type_error(
                    "toNumber() argument must be a string, number, or boolean.",
                )),
            }
        }
        "abs" => {
            one(&args, "abs")?;
            Ok(Value::Number(number(&args[0], "abs")?.abs()))
        }
        "floor" => {
            one(&args, "floor")?;
            Ok(Value::Number(number(&args[0], "floor")?.floor()))
        }
        "ceil" => {
            one(&args, "ceil")?;
            Ok(Value::Number(number(&args[0], "ceil")?.ceil()))
        }
        "sqrt" => {
            one(&args, "sqrt")?;
            let n = number(&args[0], "sqrt")?;
            if n < 0.0 {
                return Err(value_error(
                    "Cannot take the square root of a negative number.",
                ));
            }
            Ok(Value::Number(n.sqrt()))
        }
        "pow" => {
            exactly(&args, 2, "pow() takes exactly 2 arguments.")?;
            Ok(Value::Number(
                number(&args[0], "pow")?.powf(number(&args[1], "pow")?),
            ))
        }
        "min" | "max" => {
            let values: Vec<Value> = match args.as_slice() {
                [Value::Array(a)] => a.borrow().clone(),
                other => other.to_vec(),
            };
            if values.is_empty() {
                return Err(arity_error(format!(
                    "{name}() requires at least one argument."
                )));
            }
            let mut best = number(&values[0], name)?;
            for v in &values[1..] {
                let n = number(v, name)?;
                if (name == "min" && n < best) || (name == "max" && n > best) {
                    best = n;
                }
            }
            Ok(Value::Number(best))
        }
        "round" => {
            between(&args, 1, 2, "round() takes 1 or 2 arguments.")?;
            let value = number(&args[0], "round")?;
            let digits = match args.get(1) {
                Some(v) => number(v, "round")? as i32,
                None => 0,
            };
            // Half away from zero, scaled first. Deliberately not Rust's
            // `round`, so the three implementations agree on a .5 boundary.
            let factor = 10f64.powi(digits);
            let scaled = value * factor;
            let rounded = (scaled.abs() + 0.5).floor();
            Ok(Value::Number(
                if scaled < 0.0 { -rounded } else { rounded } / factor,
            ))
        }

        // -- objects -------------------------------------------------------
        "keys" => {
            let fields = object_arg(&args, "keys")?;
            Ok(Value::array(
                fields.into_iter().map(|(k, _)| Value::str(k)).collect(),
            ))
        }
        "values" => {
            let fields = object_arg(&args, "values")?;
            Ok(Value::array(fields.into_iter().map(|(_, v)| v).collect()))
        }
        "has" => {
            exactly(&args, 2, "has() takes exactly 2 arguments.")?;
            if let Value::Object(map) = &args[0] {
                let Some(key) = ObjKey::from_value(&args[1]) else {
                    return Ok(Value::Bool(false));
                };
                return Ok(Value::Bool(map.borrow().contains_key(&key)));
            }
            if let Some(fields) = object_fields(&args[0]) {
                let wanted = stringify(&args[1]);
                return Ok(Value::Bool(fields.iter().any(|(k, _)| *k == wanted)));
            }
            if let Value::Array(a) = &args[0] {
                return Ok(Value::Bool(
                    a.borrow().iter().any(|item| values_equal(item, &args[1])),
                ));
            }
            Err(type_error(
                "First argument to has() must be an array or object.",
            ))
        }
        "get" => {
            between(&args, 2, 3, "get() takes 2 or 3 arguments.")?;
            let default = args.get(2).cloned().unwrap_or(Value::Null);
            if let Value::Object(map) = &args[0] {
                let Some(key) = ObjKey::from_value(&args[1]) else {
                    return Ok(default);
                };
                return Ok(map.borrow().get(&key).cloned().unwrap_or(default));
            }
            if let Some(fields) = object_fields(&args[0]) {
                let wanted = stringify(&args[1]);
                return Ok(fields
                    .into_iter()
                    .find(|(k, _)| *k == wanted)
                    .map(|(_, v)| v)
                    .unwrap_or(default));
            }
            if let Value::Array(a) = &args[0] {
                if let Some(n) = args[1].as_number() {
                    let items = a.borrow();
                    let i = n as i64;
                    if i >= 0 && (i as usize) < items.len() {
                        return Ok(items[i as usize].clone());
                    }
                }
                return Ok(default);
            }
            // A string indexes by position like an array does, because `s[0]`
            // does too.
            if let Value::Str(s) = &args[0] {
                if let Some(n) = args[1].as_number() {
                    let chars: Vec<char> = s.chars().collect();
                    let i = n as i64;
                    if i >= 0 && (i as usize) < chars.len() {
                        return Ok(Value::str(chars[i as usize].to_string()));
                    }
                }
                return Ok(default);
            }
            // `get` is total on purpose: it is the guard idiom for a value
            // that may not be an object at all.
            Ok(default)
        }

        // -- higher order ---------------------------------------------------
        "map" | "filter" => {
            exactly(
                &args,
                2,
                format!("{name}() takes a sequence and a function."),
            )?;
            if !is_iterable(&args[0]) {
                return Err(type_error(format!(
                    "{name}() needs something iterable, not {}.",
                    type_name(&args[0])
                )));
            }
            let kind = if name == "map" {
                Transform::Map
            } else {
                Transform::Filter
            };
            // Over a generator the result is itself lazy, so `map` over an
            // endless sequence is usable -- and, just as importantly, applies
            // the function only to the items something actually pulls. Over an
            // eager sequence the answer is an array, because that is what the
            // source was: laziness is inherited, not imposed.
            if matches!(&args[0], Value::Generator(_)) {
                let source = interp.cursor(&args[0], None)?;
                return Ok(Value::Generator(Rc::new(Generator::transform(
                    format!("<generator {name}>"),
                    kind,
                    source,
                    args[1].clone(),
                ))));
            }
            let items = items_arg(interp, &args[0], name)?;
            let mut out = Vec::new();
            for item in items {
                let produced = interp.call_value(args[1].clone(), vec![item.clone()], None)?;
                if name == "map" {
                    out.push(produced);
                } else if produced.is_truthy() {
                    out.push(item);
                }
            }
            Ok(Value::array(out))
        }
        "reduce" => {
            between(
                &args,
                2,
                3,
                "reduce() takes a sequence, a function, and an optional initial value.",
            )?;
            let items = items_arg(interp, &args[0], "reduce")?;
            let (mut acc, rest) = match args.get(2) {
                Some(init) => (init.clone(), &items[..]),
                None => {
                    if items.is_empty() {
                        return Err(value_error(
                            "reduce() of an empty sequence needs an initial value.",
                        ));
                    }
                    (items[0].clone(), &items[1..])
                }
            };
            for item in rest {
                acc = interp.call_value(args[1].clone(), vec![acc, item.clone()], None)?;
            }
            Ok(acc)
        }
        "find" => {
            exactly(&args, 2, "find() takes a sequence and a function.")?;
            for item in items_arg(interp, &args[0], "find")? {
                if interp
                    .call_value(args[1].clone(), vec![item.clone()], None)?
                    .is_truthy()
                {
                    return Ok(item);
                }
            }
            Ok(Value::Null)
        }
        "some" | "every" => {
            exactly(
                &args,
                2,
                format!("{name}() takes a sequence and a function."),
            )?;
            let want_any = name == "some";
            for item in items_arg(interp, &args[0], name)? {
                let hit = interp
                    .call_value(args[1].clone(), vec![item], None)?
                    .is_truthy();
                if hit == want_any {
                    return Ok(Value::Bool(want_any));
                }
            }
            Ok(Value::Bool(!want_any))
        }
        "sort" => {
            between(
                &args,
                1,
                2,
                "sort() takes an array and an optional compare function.",
            )?;
            let array = array_arg(&args[0], "sort")?;
            let mut items = array.borrow().clone();
            match args.get(1) {
                Some(comparator) => {
                    // Insertion sort: stable, and it lets the comparator
                    // return an error without unwinding a sort in progress.
                    for i in 1..items.len() {
                        let mut j = i;
                        while j > 0 {
                            let ordering = interp.call_value(
                                comparator.clone(),
                                vec![items[j - 1].clone(), items[j].clone()],
                                None,
                            )?;
                            let Some(n) = ordering.as_number() else {
                                return Err(type_error(
                                    "sort() compare function must return a number.",
                                ));
                            };
                            if n <= 0.0 {
                                break;
                            }
                            items.swap(j - 1, j);
                            j -= 1;
                        }
                    }
                }
                None => {
                    if items.iter().all(|v| matches!(v, Value::Number(_))) {
                        items.sort_by(|a, b| {
                            a.as_number()
                                .unwrap()
                                .partial_cmp(&b.as_number().unwrap())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    } else if items.iter().all(|v| matches!(v, Value::Str(_))) {
                        items.sort_by(|a, b| a.as_str().unwrap().cmp(b.as_str().unwrap()));
                    } else {
                        return Err(value_error(
                            "sort() without a compare function needs an array of all numbers or all strings.",
                        ));
                    }
                }
            }
            Ok(Value::array(items))
        }
        "toArray" => {
            one(&args, "toArray")?;
            Ok(Value::array(interp.iterate(&args[0], None)?))
        }
        "take" => {
            exactly(&args, 2, "take() takes an iterable and a count.")?;
            let count = number(&args[1], "take")?;
            if count < 0.0 {
                return Err(value_error("take() count must not be negative."));
            }
            // Pulled one at a time rather than collected and sliced: `take`
            // over an endless generator is the whole reason it exists, and it
            // must leave the generator parked where it stopped.
            // No iterability check of its own: a non-sequence has to report
            // the language's own "Can only iterate over ..." here, which
            // building the cursor already does.
            let mut cursor = interp.cursor(&args[0], None)?;
            let mut items = Vec::new();
            while items.len() < count as usize {
                match interp.advance(&mut cursor, None)? {
                    Some(item) => items.push(item),
                    None => break,
                }
            }
            Ok(Value::array(items))
        }

        // -- driving a generator by hand ------------------------------------
        // `for`-`in` is the usual way to consume a sequence; these are for
        // when a program wants one value at a time, and `send` for when it
        // wants to pass something back in. Both report `{done, value}` rather
        // than a bare item, because "the sequence ended" and "the sequence
        // yielded null" are different answers.
        "next" | "send" => {
            let sent = if name == "next" {
                exactly(&args, 1, "next() takes a generator.")?;
                Value::Null
            } else {
                exactly(&args, 2, "send() takes a generator and a value.")?;
                args[1].clone()
            };
            let Value::Generator(generator) = &args[0] else {
                return Err(type_error(format!(
                    "{name}() needs a generator, not {}.",
                    type_name(&args[0])
                )));
            };
            // Stepping a generator that has run out is not an error -- only
            // *iterating* one is. A hand-driven consumer has no other way to
            // ask whether there is more.
            let generator = generator.clone();
            match interp.step_generator(&generator, sent, None)? {
                Some(value) => Ok(step_result(false, value)),
                None => Ok(step_result(true, Value::Null)),
            }
        }

        // -- randomness -----------------------------------------------------
        "random" => {
            one(&args, "random")?;
            let seed = number(&args[0], "random")? as i64 as u32;
            Ok(Value::Builtin(Rc::new(Builtin {
                name: "<random>",
                rng: Some(RefCell::new(if seed == 0 { 1 } else { seed })),
            })))
        }

        "print" => {
            interp.print_values(&args);
            Ok(Value::Null)
        }

        _ => Err(Signal::error(
            Kind::RuntimeError,
            format!("Unknown built-in '{name}'."),
        )),
    }
}

/// One step of a `random(seed)` stream: xorshift32 over uint32 state, matching
/// the other implementations bit for bit so a seeded program prints the same
/// numbers everywhere.
pub fn next_random(state: &RefCell<u32>, args: &[Value]) -> Eval {
    if !args.is_empty() {
        return Err(arity_error("A random generator takes no arguments."));
    }
    let mut x = *state.borrow();
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state.borrow_mut() = x;
    Ok(Value::Number(x as f64 / 4294967296.0))
}

// -- argument helpers --------------------------------------------------------

fn one(args: &[Value], who: &str) -> Result<(), Signal> {
    if args.len() != 1 {
        return Err(arity_error(format!("{who}() takes exactly one argument.")));
    }
    Ok(())
}

fn exactly(args: &[Value], n: usize, message: impl Into<String>) -> Result<(), Signal> {
    if args.len() != n {
        return Err(arity_error(message));
    }
    Ok(())
}

fn between(args: &[Value], lo: usize, hi: usize, message: impl Into<String>) -> Result<(), Signal> {
    if args.len() < lo || args.len() > hi {
        return Err(arity_error(message));
    }
    Ok(())
}

fn number(value: &Value, who: &str) -> Result<f64, Signal> {
    value
        .as_number()
        .ok_or_else(|| type_error(format!("{who}() argument must be a number.")))
}

fn array_arg(value: &Value, who: &str) -> Result<Rc<RefCell<Vec<Value>>>, Signal> {
    match value {
        Value::Array(a) => Ok(a.clone()),
        _ => Err(type_error(format!(
            "First argument to {who}() must be an array."
        ))),
    }
}

fn string_arg(value: &Value, who: &str) -> Result<String, Signal> {
    match value {
        Value::Str(s) => Ok(s.to_string()),
        _ => Err(type_error(format!(
            "First argument to {who}() must be a string."
        ))),
    }
}

fn string_arg1(value: &Value, who: &str) -> Result<String, Signal> {
    match value {
        Value::Str(s) => Ok(s.to_string()),
        _ => Err(type_error(format!("{who}() argument must be a string."))),
    }
}

fn object_arg(args: &[Value], who: &str) -> Result<Vec<(String, Value)>, Signal> {
    if args.len() != 1 {
        return Err(arity_error(format!(
            "{who}() takes exactly one object argument."
        )));
    }
    object_fields(&args[0])
        .ok_or_else(|| arity_error(format!("{who}() takes exactly one object argument.")))
}

/// The first argument of a built-in that walks a sequence: anything `for`-`in`
/// accepts, including a struct that implements `iter()`.
fn items_arg(interp: &mut Interpreter, value: &Value, who: &str) -> Result<Vec<Value>, Signal> {
    if !is_iterable(value) {
        return Err(type_error(format!(
            "{who}() needs something iterable, not {}.",
            type_name(value)
        )));
    }
    interp.iterate(value, None)
}

/// `slice`/`substring` share a start/end convention, including negative
/// indices counting from the end.
fn span(args: &[Value], len: usize) -> (usize, usize) {
    let resolve = |v: Option<&Value>, fallback: usize| -> usize {
        match v.and_then(|v| v.as_number()) {
            Some(n) => {
                let i = n as i64;
                let i = if i < 0 { len as i64 + i } else { i };
                i.clamp(0, len as i64) as usize
            }
            None => fallback,
        }
    };
    let start = resolve(args.get(1), 0);
    let end = resolve(args.get(2), len);
    (start, end.max(start))
}

fn flatten_into(items: &[Value], depth: i64, out: &mut Vec<Value>) {
    for item in items {
        match item {
            Value::Array(inner) if depth > 0 => flatten_into(&inner.borrow(), depth - 1, out),
            other => out.push(other.clone()),
        }
    }
}

/// A count: a number that is whole and not negative.
///
/// Separate from `number` because a layer size of 2.5 or -1 is a mistake worth
/// naming rather than rounding away.
fn whole(value: &Value, who: &str, what: &str) -> Result<usize, Signal> {
    match value {
        Value::Number(n) if *n >= 0.0 && n.fract() == 0.0 && n.is_finite() => Ok(*n as usize),
        Value::Number(n) => Err(value_error(format!(
            "{who}() needs {what} to be a whole number that is not negative, not {n}."
        ))),
        other => Err(type_error(format!(
            "{who}() needs {what} to be a number, not {}.",
            type_name(other)
        ))),
    }
}

/// The screen, for the built-ins that only read it.
///
/// The drawing calls take `&mut interp.screen` directly; these need the
/// interpreter borrowed immutably alongside their arguments, which is a
/// different shape and so a different helper.
fn screen_of<'a>(interp: &'a Interpreter, who: &str) -> Result<&'a crate::game::Screen, Signal> {
    interp.screen.as_ref().ok_or_else(|| {
        value_error(format!(
            "{who}() needs a screen; call gameInit(width, height, title) first."
        ))
    })
}
