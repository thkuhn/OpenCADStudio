use std::path::PathBuf;

fn main() {
    println!("Generating AEC sample project...");

    let user_folder = PathBuf::from("/home/tom/Dokumente/OpenCADStudio_Dateien/Beispielprojekt");
    match OpenCADStudio::modules::aec::engine::sample_project::generate_sample_project(&user_folder) {
        Ok(_) => println!("Successfully generated sample project in: {:?}", user_folder),
        Err(e) => eprintln!("Error generating in user folder: {:?}", e),
    }

    let repo_folder = PathBuf::from("examples/beispiel_haus");
    match OpenCADStudio::modules::aec::engine::sample_project::generate_sample_project(&repo_folder) {
        Ok(_) => println!("Successfully generated sample project in: {:?}", repo_folder),
        Err(e) => eprintln!("Error generating in repo examples: {:?}", e),
    }
}
