--INCLUDEFILE option("neovim")
-- You must enable the exrc setting in neovim for this config file to be used.
local rust_analyzer = {
    cargo = {
        --REPLACE riscv32imac-unknown-none-elf rust_target
        target = "riscv32imac-unknown-none-elf",
        allTargets = false,
    },
}
--IF option("xtensa")
--REPLACE esp toolchain-name
rust_analyzer.cargo.extraEnv = { RUST_TOOLCHAIN = "esp" }
--REPLACE esp toolchain-name
rust_analyzer.check = { extraEnv = { RUST_TOOLCHAIN = "esp" } }
rust_analyzer.server = { extraEnv = { RUST_TOOLCHAIN = "stable" } }
--ENDIF

-- Note the neovim name of the language server is rust_analyzer with an underscore.
vim.lsp.config("rust_analyzer", {
    settings = {
        ["rust-analyzer"] = rust_analyzer
    },
})
