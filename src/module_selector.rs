use esp_generate::{modules::modules_for_chip, template::GeneratorOptionItem};
use esp_metadata::Chip;

pub(crate) fn populate_module_category(chip: Chip, options: &mut [GeneratorOptionItem]) {
    let modules = modules_for_chip(chip);

    for item in options.iter_mut() {
        let GeneratorOptionItem::Category(category) = item else {
            continue;
        };
        if category.name != "module" {
            continue;
        }

        let template_opt = match category.options.first() {
            Some(GeneratorOptionItem::Option(opt)) => opt.clone(),
            _ => {
                panic!("module category must contain a placeholder !Option");
            }
        };

        category.options.clear();

        let mut opt = template_opt.clone();
        opt.name = "generic".to_string();
        opt.display_name = "Generic/unknown (no GPIO reservations)".to_string();
        opt.selection_group = "module".to_string();
        category.options.push(GeneratorOptionItem::Option(opt));

        for module in modules {
            let mut opt = template_opt.clone();
            opt.name = module.name.to_string();
            opt.display_name = module.display_name.to_string();
            opt.selection_group = "module".to_string();
            category.options.push(GeneratorOptionItem::Option(opt));
        }

        break;
    }
}
