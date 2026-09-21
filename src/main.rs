mod ui;

use ui::i18n::{Catalog, Lang};

fn main() {
    for lang in Lang::ALL {
        let t = Catalog::new(lang);
        println!("[{}]", t.lang.code());
        println!("  constant     : {}", t.login_password());
        println!("  interpolated : {}", t.sort_refuses_reorder("date"));
        println!("  selector n=1 : {}", t.rows_moved(1, "down"));
        println!("  selector n=3 : {}", t.rows_moved(3, "down"));
    }
    println!("\n{} message ids", ui::i18n::MESSAGE_IDS.len());
}
