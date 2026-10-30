//! EXPERIMENT ONLY (proto/shared-table, #44): addresses of public non-generic functions as a plain table.
#![allow(clippy::all, unused)]
pub struct Table<const N: usize>(pub [*const (); N]);
// SAFETY: addresses of functions, never written.
unsafe impl<const N: usize> Sync for Table<N> {}

#[cfg(feature = "proto-ver")]
pub static TABLE: Table<241> = Table([
    paperpaw_document::Block::source_offset as *const (), // 118 B
    paperpaw_sdk::Id::new_static as *const (), // 168 B
    paperpaw_sdk::Id::is_owned_by as *const (), // 120 B
    paperpaw_sdk::Settings::scoped_value as *const (), // 184 B
    paperpaw_sdk::Settings::get as *const (), // 800 B
    paperpaw_text::draw_text as *const (), // 4596 B
    paperpaw_i18n::Translator::format as *const (), // 1438 B
    paperpaw_i18n::Translator::plural as *const (), // 802 B
    paperpaw_text::GlyphCache::glyph as *const (), // 1402 B
    paperpaw_sdk::ui::Label::draw_from as *const (), // 1638 B
    paperpaw_text::Font::largest_size as *const (), // 696 B
    paperpaw_text::Font::kern as *const (), // 770 B
    paperpaw_text::Font::shown as *const (), // 98 B
    paperpaw_text::Font::advance as *const (), // 652 B
    paperpaw_text::Font::has_char as *const (), // 194 B
    paperpaw_text::Font::size_for as *const (), // 1400 B
    paperpaw_sdk::Fonts::get as *const (), // 78 B
    paperpaw_sdk::Fonts::role as *const (), // 902 B
    paperpaw_graphics::Canvas::fill_tint as *const (), // 894 B
    paperpaw_i18n::Catalog::get as *const (), // 978 B
    paperpaw_sdk::FontHandle::draw_toned as *const (), // 2896 B
    paperpaw_sdk::FontHandle::ink_across as *const (), // 1108 B
    paperpaw_sdk::FontHandle::line_metrics as *const (), // 408 B
    paperpaw_sdk::FontHandle::heading_line_height as *const (), // 224 B
    paperpaw_sdk::FontHandle::ink as *const (), // 786 B
    paperpaw_sdk::FontHandle::kern as *const (), // 880 B
    paperpaw_sdk::FontHandle::wrap as *const (), // 1292 B
    paperpaw_sdk::FontHandle::measure as *const (), // 474 B
    paperpaw_sdk::FontHandle::size_for as *const (), // 114 B
    paperpaw_platform::geometry::Rect::intersection as *const (), // 124 B
    paperpaw_text::BitmapFont::glyph as *const (), // 748 B
    paperpaw_text::charset::stand_in as *const (), // 208 B
    paperpaw_sdk::ui::draw_label as *const (), // 578 B
    paperpaw_sdk::ui::heading_height as *const (), // 170 B
    paperpaw_sdk::ui::fit as *const (), // 762 B
    paperpaw_sdk::ui::draw_arrow as *const (), // 992 B
    paperpaw_xml::local_name as *const (), // 78 B
    paperpaw_i18n::Translator::tr as *const (), // 1606 B
    paperpaw_xml::Tokenizer::try_feed as *const (), // 630 B
    paperpaw_sdk::ask::ChoiceQuestion::options as *const (), // 186 B
    paperpaw_sdk::Params::get as *const (), // 94 B
    paperpaw_sdk::Formats::for_file as *const (), // 830 B
    paperpaw_sdk::Formats::for_path as *const (), // 200 B
    paperpaw_sdk::Formats::may_sniff as *const (), // 268 B
    paperpaw_graphics::Canvas::fill_round_rect as *const (), // 362 B
    paperpaw_graphics::Canvas::stroke_round_rect as *const (), // 708 B
    paperpaw_sdk::Cx::request_redraw_area as *const (), // 112 B
    paperpaw_platform::geometry::Rect::union as *const (), // 202 B
    paperpaw_sdk::ui::ListView::focus_change as *const (), // 126 B
    paperpaw_sdk::ui::ListView::row_rect as *const (), // 106 B
    paperpaw_platform::storage::is_reserved_name as *const (), // 544 B
    paperpaw_sdk::web::clean_name as *const (), // 1568 B
    paperpaw_sdk::web::host as *const (), // 372 B
    paperpaw_sdk::files::copy as *const (), // 1080 B
    paperpaw_sdk::files::join as *const (), // 166 B
    paperpaw_sdk::files::move_to as *const (), // 270 B
    paperpaw_sdk::files::free_path as *const (), // 530 B
    paperpaw_sdk::files::is_within as *const (), // 190 B
    paperpaw_sdk::library::folder as *const (), // 66 B
    paperpaw_graphics::round::corner_inset as *const (), // 1164 B
    paperpaw_json::string as *const (), // 1092 B
    paperpaw_config::Value::as_str as *const (), // 1142 B
    paperpaw_config::Value::as_u32 as *const (), // 170 B
    paperpaw_sdk::data::Format::load as *const (), // 510 B
    paperpaw_sdk::secrets::Visible::get as *const (), // 190 B
    paperpaw_digest::md5::Md5::update as *const (), // 600 B
    paperpaw_digest::sha1::Sha1::update as *const (), // 194 B
    paperpaw_digest::hex::push as *const (), // 142 B
    paperpaw_sdk::read_file as *const (), // 370 B
    paperpaw_sdk::ui::draw_icon as *const (), // 232 B
    paperpaw_sdk::ui::button_height_for as *const (), // 258 B
    <paperpaw_sdk::Version as core::fmt::Display>::fmt as *const (), // 246 B
    paperpaw_sdk::ui::Row::draw as *const (), // 4756 B
    paperpaw_sdk::time::LocalTime::from_seconds as *const (), // 748 B
    paperpaw_graphics::Canvas::stroke_rect as *const (), // 238 B
    paperpaw_graphics::Canvas::clipped as *const (), // 148 B
    paperpaw_sdk::ui::UiStyle::noted_row as *const (), // 144 B
    paperpaw_sdk::time::TimeZone::parse as *const (), // 314 B
    paperpaw_sdk::time::local_time as *const (), // 588 B
    paperpaw_sdk::time::days_from_civil as *const (), // 354 B
    paperpaw_i18n::Translator::get as *const (), // 120 B
    paperpaw_sdk::ui::ListChange::request as *const (), // 174 B
    paperpaw_sdk::ui::Header::tap as *const (), // 428 B
    paperpaw_sdk::ui::Header::draw as *const (), // 4702 B
    paperpaw_sdk::ui::UiStyle::new as *const (), // 2892 B
    paperpaw_sdk::install::Installer::cancel as *const (), // 270 B
    paperpaw_sdk::install::Installer::progress as *const (), // 148 B
    paperpaw_digest::sha256::Sha256::update as *const (), // 190 B
    paperpaw_text::BitmapFont::is_pack as *const (), // 108 B
    paperpaw_sdk::HttpRequest::with as *const (), // 222 B
    paperpaw_sdk::ui::ListView::hit as *const (), // 204 B
    paperpaw_sdk::ui::ListView::navigate as *const (), // 564 B
    paperpaw_sdk::ui::ListView::set_focus as *const (), // 124 B
    paperpaw_sdk::ui::Ui::note_line as *const (), // 1910 B
    paperpaw_sdk::Id::parse as *const (), // 246 B
    paperpaw_digest::hex::encode as *const (), // 218 B
    paperpaw_sdk::web::join_url as *const (), // 3446 B
    paperpaw_sdk::files::size as *const (), // 554 B
    paperpaw_sdk::ui::tr_input as *const (), // 248 B
    <paperpaw_sdk::replies::HttpReply as paperpaw_sdk::event::EventData>::from_params as *const (), // 1958 B
    paperpaw_digest::md5::Md5::hex as *const (), // 364 B
    paperpaw_json::stream::read_file as *const (), // 1964 B
    paperpaw_sdk::files::without_bom as *const (), // 94 B
    paperpaw_sdk::files::for_each_line as *const (), // 758 B
    paperpaw_sdk::files::has_extension as *const (), // 104 B
    paperpaw_xml::decode as *const (), // 1366 B
    <paperpaw_sdk::replies::Typed as paperpaw_sdk::event::EventData>::from_params as *const (), // 642 B
    <paperpaw_sdk::builtin::events::SettingChanged as paperpaw_sdk::event::EventData>::from_params as *const (), // 422 B
    paperpaw_sdk::ui::Row::height as *const (), // 448 B
    paperpaw_sdk::ui::Rows::hit as *const (), // 212 B
    paperpaw_sdk::ui::Rows::new as *const (), // 710 B
    paperpaw_sdk::ui::Rows::step as *const (), // 226 B
    paperpaw_sdk::ui::Rows::turn as *const (), // 266 B
    paperpaw_sdk::ui::Rows::rect_of as *const (), // 168 B
    paperpaw_sdk::ui::Rows::visible as *const (), // 550 B
    paperpaw_sdk::ui::Rows::set_focus as *const (), // 320 B
    paperpaw_sdk::Fonts::bound as *const (), // 764 B
    paperpaw_sdk::MenuView::items as *const (), // 1358 B
    paperpaw_sdk::Actions::get as *const (), // 398 B
    paperpaw_sdk::Menus::is_section as *const (), // 92 B
    paperpaw_sdk::Menus::contributed_ids as *const (), // 1280 B
    paperpaw_sdk::Menus::icon as *const (), // 166 B
    paperpaw_sdk::ui::Ui::heading as *const (), // 632 B
    paperpaw_sdk::ui::Ui::row_icon as *const (), // 470 B
    paperpaw_sdk::ui::icon_room as *const (), // 142 B
    paperpaw_sdk::data::Format::version_of as *const (), // 410 B
    paperpaw_document::Block::text_index as *const (), // 222 B
    paperpaw_sdk::annotations::file as *const (), // 194 B
    paperpaw_sdk::annotations::snippet as *const (), // 636 B
    paperpaw_sdk::annotations::one_line as *const (), // 554 B
    paperpaw_sdk::library::key as *const (), // 120 B
    paperpaw_json::text as *const (), // 96 B
    paperpaw_json::object as *const (), // 2244 B
    paperpaw_sdk::contribution::Registrar::add_action as *const (), // 108 B
    paperpaw_sdk::Formats::open_with as *const (), // 1292 B
    paperpaw_sdk::secrets::SecretChange::keep as *const (), // 250 B
    paperpaw_sdk::ui::Ui::message as *const (), // 1328 B
    paperpaw_digest::md5::of as *const (), // 122 B
    paperpaw_sdk::reading::open_options as *const (), // 1268 B
    paperpaw_sdk::ui::message_height as *const (), // 316 B
    <paperpaw_sdk::builtin::events::Reading as paperpaw_sdk::event::EventData>::from_params as *const (), // 1956 B
    paperpaw_sdk::ui::reorder::Reorder::draw as *const (), // 314 B
    paperpaw_sdk::ui::reorder::Reorder::draw_grip as *const (), // 646 B
    paperpaw_sdk::Actions::contains as *const (), // 78 B
    paperpaw_sdk::ui::ListView::move_focus as *const (), // 262 B
    paperpaw_sdk::Settings::spec as *const (), // 116 B
    paperpaw_xml::Attrs::get as *const (), // 1244 B
    paperpaw_sdk::contribution::Registrar::add_settings as *const (), // 532 B
    paperpaw_sdk::widgets::Frame::of as *const (), // 92 B
    paperpaw_graphics::Canvas::fill_round_shaded as *const (), // 786 B
    paperpaw_sdk::widgets::Wrapped::draw as *const (), // 990 B
    paperpaw_sdk::widgets::Align::x as *const (), // 130 B
    paperpaw_sdk::widgets::align as *const (), // 188 B
    paperpaw_sdk::widgets::line_height as *const (), // 218 B
    paperpaw_sdk::widgets::widget_look as *const (), // 804 B
    paperpaw_sdk::widgets::heading_line_height as *const (), // 214 B
    paperpaw_config::quote as *const (), // 1042 B
    paperpaw_sdk::SettingKind::accepts as *const (), // 384 B
    paperpaw_graphics::Framebuffer::view_rows as *const (), // 322 B
    paperpaw_graphics::diffuse::Atkinson::new as *const (), // 322 B
    paperpaw_image::GrayFile::read_rows as *const (), // 136 B
    paperpaw_image::AlphaFile::new as *const (), // 134 B
    paperpaw_image::ColorFile::read_rows as *const (), // 146 B
    paperpaw_image::Format::detect as *const (), // 128 B
    paperpaw_platform::display::FrameView::pixel as *const (), // 362 B
    paperpaw_graphics::Canvas::draw_rgb_row as *const (), // 700 B
    paperpaw_graphics::Canvas::draw_gray_row as *const (), // 456 B
    paperpaw_graphics::Canvas::clip as *const (), // 324 B
    paperpaw_graphics::Canvas::describe as *const (), // 322 B
    paperpaw_image::Downscaler::new as *const (), // 1078 B
    paperpaw_image::Downscaler::row as *const (), // 306 B
    paperpaw_image::Downscaler::finish as *const (), // 82 B
    paperpaw_image::ImageFile::new as *const (), // 156 B
    paperpaw_image::ImageFile::open as *const (), // 502 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::row as *const (), // 172 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::start as *const (), // 246 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::row_rgb as *const (), // 218 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::row_alpha as *const (), // 200 B
    paperpaw_sdk::Id::namespace as *const (), // 72 B
    paperpaw_sdk::ui::list_step as *const (), // 338 B
    paperpaw_sdk::contribution::Registrar::add_widget as *const (), // 112 B
    paperpaw_sdk::contribution::Registrar::add_screens as *const (), // 462 B
    paperpaw_sdk::contribution::Registrar::add_status_item as *const (), // 112 B
    paperpaw_graphics::Canvas::draw_line as *const (), // 1324 B
    paperpaw_digest::hex::decode as *const (), // 288 B
    paperpaw_sdk::ui::fit_pair as *const (), // 270 B
    paperpaw_sdk::widgets::draw_frame as *const (), // 432 B
    paperpaw_sdk::widgets::fit_line as *const (), // 336 B
    <paperpaw_sdk::replies::Confirmed as paperpaw_sdk::event::EventData>::from_params as *const (), // 158 B
    paperpaw_sdk::web::encode as *const (), // 436 B
    paperpaw_sdk::contribution::Registrar::add_actions as *const (), // 612 B
    paperpaw_sdk::contribution::Registrar::add_decorator as *const (), // 112 B
    paperpaw_sdk::ServerResponse::new as *const (), // 146 B
    paperpaw_sdk::help::add as *const (), // 538 B
    paperpaw_sdk::served::json_string as *const (), // 884 B
    paperpaw_sdk::served::param as *const (), // 576 B
    paperpaw_sdk::ui::step_focus as *const (), // 122 B
    paperpaw_sdk::files::same_path as *const (), // 344 B
    paperpaw_json::stream::Parser::feed as *const (), // 1342 B
    paperpaw_config::set as *const (), // 3264 B
    paperpaw_sdk::logo::Logo::fit as *const (), // 142 B
    paperpaw_sdk::library::catalog::Changes::apply as *const (), // 292 B
    paperpaw_image::Fit::size as *const (), // 92 B
    paperpaw_sdk::library::catalog::Query::finds as *const (), // 508 B
    paperpaw_sdk::Formats::supports_file as *const (), // 110 B
    paperpaw_sdk::library::Book::shown_title as *const (), // 1618 B
    paperpaw_sdk::ui::Ui::new as *const (), // 92 B
    paperpaw_sdk::ui::Thumbnail::new as *const (), // 848 B
    paperpaw_sdk::library::catalog::Record::of as *const (), // 804 B
    paperpaw_sdk::library::catalog::Record::matches as *const (), // 1024 B
    paperpaw_sdk::time::civil_from_days as *const (), // 538 B
    paperpaw_sdk::files::load_text_for_update as *const (), // 244 B
    paperpaw_sdk::image::picture_from_file as *const (), // 566 B
    paperpaw_sdk::library::changed_book as *const (), // 222 B
    paperpaw_sdk::library::book as *const (), // 242 B
    paperpaw_sdk::ui::icon_width as *const (), // 66 B
    paperpaw_sdk::ui::draw_image_over as *const (), // 4694 B
    paperpaw_sdk::library::index::catalog_version as *const (), // 596 B
    paperpaw_sdk::library::index::opened as *const (), // 294 B
    paperpaw_sdk::library::catalog::series_order as *const (), // 472 B
    paperpaw_sdk::library::catalog::each_at as *const (), // 204 B
    paperpaw_sdk::library::catalog::read_page as *const (), // 834 B
    paperpaw_sdk::library::is_book_path as *const (), // 268 B
    paperpaw_sdk::library::service::is_book_path as *const (), // 268 B
    paperpaw_sdk::library::service as *const (), // 272 B
    paperpaw_sdk::library::service::service as *const (), // 272 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::index as *const (), // 596 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::row as *const (), // 200 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::start as *const (), // 250 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::row_alpha as *const (), // 200 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::can_delete as *const (), // 78 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::is_current as *const (), // 80 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::keep_index as *const (), // 908 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::kept_index as *const (), // 1438 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::set_finished as *const (), // 6630 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::catalog_state as *const (), // 294 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::set_favourite as *const (), // 724 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::query as *const (), // 494 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::delete as *const (), // 644 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::record as *const (), // 256 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::refresh as *const (), // 468 B
]);

#[cfg(feature = "proto-stress")]
pub static TABLE: Table<297> = Table([
    paperpaw_document::Block::source_offset as *const (), // 118 B
    paperpaw_sdk::Id::new_static as *const (), // 168 B
    paperpaw_sdk::Id::is_owned_by as *const (), // 120 B
    paperpaw_sdk::Settings::scoped_value as *const (), // 184 B
    paperpaw_sdk::Settings::get as *const (), // 800 B
    paperpaw_text::draw_text as *const (), // 4596 B
    paperpaw_i18n::Translator::format as *const (), // 1438 B
    paperpaw_i18n::Translator::plural as *const (), // 802 B
    paperpaw_text::GlyphCache::glyph as *const (), // 1402 B
    paperpaw_sdk::ui::Label::draw_from as *const (), // 1638 B
    paperpaw_text::Font::largest_size as *const (), // 696 B
    paperpaw_text::Font::kern as *const (), // 770 B
    paperpaw_text::Font::shown as *const (), // 98 B
    paperpaw_text::Font::advance as *const (), // 652 B
    paperpaw_text::Font::has_char as *const (), // 194 B
    paperpaw_text::Font::size_for as *const (), // 1400 B
    paperpaw_sdk::Fonts::get as *const (), // 78 B
    paperpaw_sdk::Fonts::role as *const (), // 902 B
    paperpaw_graphics::Canvas::fill_tint as *const (), // 894 B
    paperpaw_i18n::Catalog::get as *const (), // 978 B
    paperpaw_sdk::FontHandle::draw_toned as *const (), // 2896 B
    paperpaw_sdk::FontHandle::ink_across as *const (), // 1108 B
    paperpaw_sdk::FontHandle::line_metrics as *const (), // 408 B
    paperpaw_sdk::FontHandle::heading_line_height as *const (), // 224 B
    paperpaw_sdk::FontHandle::ink as *const (), // 786 B
    paperpaw_sdk::FontHandle::kern as *const (), // 880 B
    paperpaw_sdk::FontHandle::wrap as *const (), // 1292 B
    paperpaw_sdk::FontHandle::measure as *const (), // 474 B
    paperpaw_sdk::FontHandle::size_for as *const (), // 114 B
    paperpaw_platform::geometry::Rect::intersection as *const (), // 124 B
    paperpaw_text::BitmapFont::glyph as *const (), // 748 B
    paperpaw_text::charset::stand_in as *const (), // 208 B
    paperpaw_sdk::ui::draw_label as *const (), // 578 B
    paperpaw_sdk::ui::heading_height as *const (), // 170 B
    paperpaw_sdk::ui::fit as *const (), // 762 B
    paperpaw_sdk::ui::draw_arrow as *const (), // 992 B
    paperpaw_xml::local_name as *const (), // 78 B
    paperpaw_i18n::Translator::tr as *const (), // 1606 B
    paperpaw_xml::Tokenizer::try_feed as *const (), // 630 B
    paperpaw_sdk::ask::ChoiceQuestion::options as *const (), // 186 B
    paperpaw_sdk::Params::get as *const (), // 94 B
    paperpaw_sdk::Formats::for_file as *const (), // 830 B
    paperpaw_sdk::Formats::for_path as *const (), // 200 B
    paperpaw_sdk::Formats::may_sniff as *const (), // 268 B
    paperpaw_graphics::Canvas::fill_round_rect as *const (), // 362 B
    paperpaw_graphics::Canvas::stroke_round_rect as *const (), // 708 B
    paperpaw_sdk::Cx::request_redraw_area as *const (), // 112 B
    paperpaw_platform::geometry::Rect::union as *const (), // 202 B
    paperpaw_sdk::ui::ListView::focus_change as *const (), // 126 B
    paperpaw_sdk::ui::ListView::row_rect as *const (), // 106 B
    paperpaw_platform::storage::is_reserved_name as *const (), // 544 B
    paperpaw_sdk::web::clean_name as *const (), // 1568 B
    paperpaw_sdk::web::host as *const (), // 372 B
    paperpaw_sdk::files::copy as *const (), // 1080 B
    paperpaw_sdk::files::join as *const (), // 166 B
    paperpaw_sdk::files::move_to as *const (), // 270 B
    paperpaw_sdk::files::free_path as *const (), // 530 B
    paperpaw_sdk::files::is_within as *const (), // 190 B
    paperpaw_sdk::library::folder as *const (), // 66 B
    paperpaw_graphics::round::corner_inset as *const (), // 1164 B
    paperpaw_json::string as *const (), // 1092 B
    paperpaw_config::Value::as_str as *const (), // 1142 B
    paperpaw_config::Value::as_u32 as *const (), // 170 B
    paperpaw_sdk::data::Format::load as *const (), // 510 B
    paperpaw_sdk::secrets::Visible::get as *const (), // 190 B
    paperpaw_digest::md5::Md5::update as *const (), // 600 B
    paperpaw_digest::sha1::Sha1::update as *const (), // 194 B
    paperpaw_digest::hex::push as *const (), // 142 B
    paperpaw_sdk::read_file as *const (), // 370 B
    paperpaw_sdk::ui::draw_icon as *const (), // 232 B
    paperpaw_sdk::ui::button_height_for as *const (), // 258 B
    <paperpaw_sdk::Version as core::fmt::Display>::fmt as *const (), // 246 B
    paperpaw_sdk::ui::Row::draw as *const (), // 4756 B
    paperpaw_sdk::time::LocalTime::from_seconds as *const (), // 748 B
    paperpaw_graphics::Canvas::stroke_rect as *const (), // 238 B
    paperpaw_graphics::Canvas::clipped as *const (), // 148 B
    paperpaw_sdk::ui::UiStyle::noted_row as *const (), // 144 B
    paperpaw_sdk::time::TimeZone::parse as *const (), // 314 B
    paperpaw_sdk::time::local_time as *const (), // 588 B
    paperpaw_sdk::time::days_from_civil as *const (), // 354 B
    paperpaw_i18n::Translator::get as *const (), // 120 B
    paperpaw_sdk::ui::ListChange::request as *const (), // 174 B
    paperpaw_sdk::ui::Header::tap as *const (), // 428 B
    paperpaw_sdk::ui::Header::draw as *const (), // 4702 B
    paperpaw_sdk::ui::UiStyle::new as *const (), // 2892 B
    paperpaw_sdk::install::Installer::cancel as *const (), // 270 B
    paperpaw_sdk::install::Installer::progress as *const (), // 148 B
    paperpaw_digest::sha256::Sha256::update as *const (), // 190 B
    paperpaw_text::BitmapFont::is_pack as *const (), // 108 B
    paperpaw_sdk::HttpRequest::with as *const (), // 222 B
    paperpaw_sdk::ui::ListView::hit as *const (), // 204 B
    paperpaw_sdk::ui::ListView::navigate as *const (), // 564 B
    paperpaw_sdk::ui::ListView::set_focus as *const (), // 124 B
    paperpaw_sdk::ui::Ui::note_line as *const (), // 1910 B
    paperpaw_sdk::Id::parse as *const (), // 246 B
    paperpaw_digest::hex::encode as *const (), // 218 B
    paperpaw_sdk::web::join_url as *const (), // 3446 B
    paperpaw_sdk::files::size as *const (), // 554 B
    paperpaw_sdk::ui::tr_input as *const (), // 248 B
    <paperpaw_sdk::replies::HttpReply as paperpaw_sdk::event::EventData>::from_params as *const (), // 1958 B
    paperpaw_digest::md5::Md5::hex as *const (), // 364 B
    paperpaw_json::stream::read_file as *const (), // 1964 B
    paperpaw_sdk::files::without_bom as *const (), // 94 B
    paperpaw_sdk::files::for_each_line as *const (), // 758 B
    paperpaw_sdk::files::has_extension as *const (), // 104 B
    paperpaw_xml::decode as *const (), // 1366 B
    <paperpaw_sdk::replies::Typed as paperpaw_sdk::event::EventData>::from_params as *const (), // 642 B
    <paperpaw_sdk::builtin::events::SettingChanged as paperpaw_sdk::event::EventData>::from_params as *const (), // 422 B
    paperpaw_sdk::ui::Row::height as *const (), // 448 B
    paperpaw_sdk::ui::Rows::hit as *const (), // 212 B
    paperpaw_sdk::ui::Rows::new as *const (), // 710 B
    paperpaw_sdk::ui::Rows::step as *const (), // 226 B
    paperpaw_sdk::ui::Rows::turn as *const (), // 266 B
    paperpaw_sdk::ui::Rows::rect_of as *const (), // 168 B
    paperpaw_sdk::ui::Rows::visible as *const (), // 550 B
    paperpaw_sdk::ui::Rows::set_focus as *const (), // 320 B
    paperpaw_sdk::Fonts::bound as *const (), // 764 B
    paperpaw_sdk::MenuView::items as *const (), // 1358 B
    paperpaw_sdk::Actions::get as *const (), // 398 B
    paperpaw_sdk::Menus::is_section as *const (), // 92 B
    paperpaw_sdk::Menus::contributed_ids as *const (), // 1280 B
    paperpaw_sdk::Menus::icon as *const (), // 166 B
    paperpaw_sdk::ui::Ui::heading as *const (), // 632 B
    paperpaw_sdk::ui::Ui::row_icon as *const (), // 470 B
    paperpaw_sdk::ui::icon_room as *const (), // 142 B
    paperpaw_sdk::data::Format::version_of as *const (), // 410 B
    paperpaw_document::Block::text_index as *const (), // 222 B
    paperpaw_sdk::annotations::file as *const (), // 194 B
    paperpaw_sdk::annotations::snippet as *const (), // 636 B
    paperpaw_sdk::annotations::one_line as *const (), // 554 B
    paperpaw_sdk::library::key as *const (), // 120 B
    paperpaw_json::text as *const (), // 96 B
    paperpaw_json::object as *const (), // 2244 B
    paperpaw_sdk::contribution::Registrar::add_action as *const (), // 108 B
    paperpaw_sdk::Formats::open_with as *const (), // 1292 B
    paperpaw_sdk::secrets::SecretChange::keep as *const (), // 250 B
    paperpaw_sdk::ui::Ui::message as *const (), // 1328 B
    paperpaw_digest::md5::of as *const (), // 122 B
    paperpaw_sdk::reading::open_options as *const (), // 1268 B
    paperpaw_sdk::ui::message_height as *const (), // 316 B
    <paperpaw_sdk::builtin::events::Reading as paperpaw_sdk::event::EventData>::from_params as *const (), // 1956 B
    paperpaw_sdk::ui::reorder::Reorder::draw as *const (), // 314 B
    paperpaw_sdk::ui::reorder::Reorder::draw_grip as *const (), // 646 B
    paperpaw_sdk::Actions::contains as *const (), // 78 B
    paperpaw_sdk::ui::ListView::move_focus as *const (), // 262 B
    paperpaw_sdk::Settings::spec as *const (), // 116 B
    paperpaw_xml::Attrs::get as *const (), // 1244 B
    paperpaw_sdk::contribution::Registrar::add_settings as *const (), // 532 B
    paperpaw_sdk::widgets::Frame::of as *const (), // 92 B
    paperpaw_graphics::Canvas::fill_round_shaded as *const (), // 786 B
    paperpaw_sdk::widgets::Wrapped::draw as *const (), // 990 B
    paperpaw_sdk::widgets::Align::x as *const (), // 130 B
    paperpaw_sdk::widgets::align as *const (), // 188 B
    paperpaw_sdk::widgets::line_height as *const (), // 218 B
    paperpaw_sdk::widgets::widget_look as *const (), // 804 B
    paperpaw_sdk::widgets::heading_line_height as *const (), // 214 B
    paperpaw_config::quote as *const (), // 1042 B
    paperpaw_sdk::SettingKind::accepts as *const (), // 384 B
    paperpaw_graphics::Framebuffer::view_rows as *const (), // 322 B
    paperpaw_graphics::diffuse::Atkinson::new as *const (), // 322 B
    paperpaw_image::GrayFile::read_rows as *const (), // 136 B
    paperpaw_image::AlphaFile::new as *const (), // 134 B
    paperpaw_image::ColorFile::read_rows as *const (), // 146 B
    paperpaw_image::Format::detect as *const (), // 128 B
    paperpaw_platform::display::FrameView::pixel as *const (), // 362 B
    paperpaw_graphics::Canvas::draw_rgb_row as *const (), // 700 B
    paperpaw_graphics::Canvas::draw_gray_row as *const (), // 456 B
    paperpaw_graphics::Canvas::clip as *const (), // 324 B
    paperpaw_graphics::Canvas::describe as *const (), // 322 B
    paperpaw_image::Downscaler::new as *const (), // 1078 B
    paperpaw_image::Downscaler::row as *const (), // 306 B
    paperpaw_image::Downscaler::finish as *const (), // 82 B
    paperpaw_image::ImageFile::new as *const (), // 156 B
    paperpaw_image::ImageFile::open as *const (), // 502 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::row as *const (), // 172 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::start as *const (), // 246 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::row_rgb as *const (), // 218 B
    <paperpaw_image::GrayWriter as paperpaw_image::ImageSink>::row_alpha as *const (), // 200 B
    paperpaw_sdk::Id::namespace as *const (), // 72 B
    paperpaw_sdk::ui::list_step as *const (), // 338 B
    paperpaw_sdk::contribution::Registrar::add_widget as *const (), // 112 B
    paperpaw_sdk::contribution::Registrar::add_screens as *const (), // 462 B
    paperpaw_sdk::contribution::Registrar::add_status_item as *const (), // 112 B
    paperpaw_graphics::Canvas::draw_line as *const (), // 1324 B
    paperpaw_digest::hex::decode as *const (), // 288 B
    paperpaw_sdk::ui::fit_pair as *const (), // 270 B
    paperpaw_sdk::widgets::draw_frame as *const (), // 432 B
    paperpaw_sdk::widgets::fit_line as *const (), // 336 B
    <paperpaw_sdk::replies::Confirmed as paperpaw_sdk::event::EventData>::from_params as *const (), // 158 B
    paperpaw_sdk::web::encode as *const (), // 436 B
    paperpaw_sdk::contribution::Registrar::add_actions as *const (), // 612 B
    paperpaw_sdk::contribution::Registrar::add_decorator as *const (), // 112 B
    paperpaw_sdk::ServerResponse::new as *const (), // 146 B
    paperpaw_sdk::help::add as *const (), // 538 B
    paperpaw_sdk::served::json_string as *const (), // 884 B
    paperpaw_sdk::served::param as *const (), // 576 B
    paperpaw_sdk::ui::step_focus as *const (), // 122 B
    paperpaw_sdk::files::same_path as *const (), // 344 B
    paperpaw_json::stream::Parser::feed as *const (), // 1342 B
    paperpaw_config::set as *const (), // 3264 B
    paperpaw_sdk::logo::Logo::fit as *const (), // 142 B
    paperpaw_sdk::library::catalog::Changes::apply as *const (), // 292 B
    paperpaw_image::Fit::size as *const (), // 92 B
    paperpaw_sdk::library::catalog::Query::finds as *const (), // 508 B
    paperpaw_sdk::Formats::supports_file as *const (), // 110 B
    paperpaw_sdk::library::Book::shown_title as *const (), // 1618 B
    paperpaw_sdk::ui::Ui::new as *const (), // 92 B
    paperpaw_sdk::ui::Thumbnail::new as *const (), // 848 B
    paperpaw_sdk::library::catalog::Record::of as *const (), // 804 B
    paperpaw_sdk::library::catalog::Record::matches as *const (), // 1024 B
    paperpaw_sdk::time::civil_from_days as *const (), // 538 B
    paperpaw_sdk::files::load_text_for_update as *const (), // 244 B
    paperpaw_sdk::image::picture_from_file as *const (), // 566 B
    paperpaw_sdk::library::changed_book as *const (), // 222 B
    paperpaw_sdk::library::book as *const (), // 242 B
    paperpaw_sdk::ui::icon_width as *const (), // 66 B
    paperpaw_sdk::ui::draw_image_over as *const (), // 4694 B
    paperpaw_sdk::library::index::catalog_version as *const (), // 596 B
    paperpaw_sdk::library::index::opened as *const (), // 294 B
    paperpaw_sdk::library::catalog::series_order as *const (), // 472 B
    paperpaw_sdk::library::catalog::each_at as *const (), // 204 B
    paperpaw_sdk::library::catalog::read_page as *const (), // 834 B
    paperpaw_sdk::library::is_book_path as *const (), // 268 B
    paperpaw_sdk::library::service::is_book_path as *const (), // 268 B
    paperpaw_sdk::library::service as *const (), // 272 B
    paperpaw_sdk::library::service::service as *const (), // 272 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::index as *const (), // 596 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::row as *const (), // 200 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::start as *const (), // 250 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::row_alpha as *const (), // 200 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::can_delete as *const (), // 78 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::is_current as *const (), // 80 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::keep_index as *const (), // 908 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::kept_index as *const (), // 1438 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::set_finished as *const (), // 6630 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::catalog_state as *const (), // 294 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::set_favourite as *const (), // 724 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::query as *const (), // 494 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::delete as *const (), // 644 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::record as *const (), // 256 B
    <paperpaw_sdk::library::service::Kept as paperpaw_sdk::library::service::Library>::refresh as *const (), // 468 B
    paperpaw_sdk::Env::plugin_usage as *const (), // 562 B
    paperpaw_sdk::ui::ListView::paging as *const (), // 124 B
    paperpaw_sdk::web::file_name as *const (), // 942 B
    paperpaw_sdk::files::name as *const (), // 282 B
    paperpaw_sdk::readers::name as *const (), // 282 B
    paperpaw_json::raw_member as *const (), // 728 B
    paperpaw_json::string_array as *const (), // 460 B
    paperpaw_sdk::Env::device_name as *const (), // 732 B
    paperpaw_sdk::Env::plugin_dir as *const (), // 66 B
    paperpaw_sdk::library::request_catalog_anew as *const (), // 170 B
    paperpaw_sdk::time::UnixTime::iso8601 as *const (), // 936 B
    paperpaw_sdk::files::read_text as *const (), // 490 B
    paperpaw_sdk::FontSet::add as *const (), // 108 B
    paperpaw_sdk::ui::Ui::empty as *const (), // 3474 B
    paperpaw_sdk::time::LocalTime::to_seconds as *const (), // 116 B
    paperpaw_sdk::time::UnixTime::parse_iso8601 as *const (), // 2100 B
    paperpaw_sdk::time::UnixTime::parse_http_date as *const (), // 1242 B
    paperpaw_sdk::keys::InputSpec::key as *const (), // 76 B
    paperpaw_sdk::annotations::relocate as *const (), // 1422 B
    paperpaw_sdk::time::local_seconds as *const (), // 974 B
    paperpaw_sdk::files::list as *const (), // 278 B
    paperpaw_sdk::replies::HttpReply::text as *const (), // 418 B
    paperpaw_sdk::MenuView::ids as *const (), // 882 B
    paperpaw_sdk::MenuView::menu_ids as *const (), // 904 B
    paperpaw_sdk::Menus::default_ids as *const (), // 142 B
    paperpaw_sdk::ui::slider::shows as *const (), // 136 B
    paperpaw_sdk::widgets::shows as *const (), // 136 B
    paperpaw_graphics::Canvas::stroke_round as *const (), // 676 B
    <paperpaw_sdk::builtin::events::ReadToday as paperpaw_sdk::event::EventData>::from_params as *const (), // 404 B
    paperpaw_graphics::Canvas::fill_ring as *const (), // 1998 B
    paperpaw_sdk::web::base_url as *const (), // 1148 B
    paperpaw_sdk::time::week_start as *const (), // 156 B
    paperpaw_graphics::round::sin_cos as *const (), // 292 B
    paperpaw_sdk::widgets::draw_message as *const (), // 1118 B
    paperpaw_sdk::widgets::Wrapped::fitting as *const (), // 598 B
    paperpaw_sdk::files::put_in_place as *const (), // 336 B
    paperpaw_sdk::words::fold as *const (), // 122 B
    paperpaw_sdk::widgets::draw_caption as *const (), // 490 B
    paperpaw_sdk::annotations::changed as *const (), // 232 B
    paperpaw_sdk::library::changed as *const (), // 232 B
    paperpaw_sdk::widgets::changed as *const (), // 232 B
    paperpaw_sdk::logo::Logo::read as *const (), // 742 B
    paperpaw_graphics::Canvas::fill_round as *const (), // 350 B
    paperpaw_graphics::Canvas::fill_shaded as *const (), // 114 B
    paperpaw_sdk::time::TimeZone::offset_at as *const (), // 276 B
    paperpaw_sdk::ui::Thumbnail::draw as *const (), // 638 B
    paperpaw_sdk::time::day_of_year as *const (), // 232 B
    paperpaw_sdk::time::twelve_hours as *const (), // 66 B
    paperpaw_sdk::time::clock_of as *const (), // 260 B
    paperpaw_sdk::time::iso_week as *const (), // 964 B
    paperpaw_graphics::round::point_at as *const (), // 410 B
    paperpaw_sdk::library::catalog::changes as *const (), // 440 B
    paperpaw_sdk::widgets::ink_baseline as *const (), // 302 B
    paperpaw_sdk::widgets::draw_line as *const (), // 772 B
    <paperpaw_image::ColorWriter as paperpaw_image::ImageSink>::row_rgb as *const (), // 172 B
    <paperpaw_sdk::Params as paperpaw_sdk::event::EventData>::from_params as *const (), // 76 B
]);

pub fn touch(n: u64) -> usize {
    let table = core::hint::black_box(&TABLE.0);
    // SAFETY: within the table.
    unsafe { core::ptr::read_volatile(table.as_ptr().add((n as usize) % table.len())) as usize }
}
