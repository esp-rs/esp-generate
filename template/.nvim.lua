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
--REPLACE esp rust_toolchain
rust_analyzer.cargo.extraEnv = { RUSTUP_TOOLCHAIN = "esp" }
rust_analyzer.server = { extraEnv = { RUSTUP_TOOLCHAIN = "stable" } }
--ENDIF

--IF option("neovim-rustaceanvim")
vim.lsp.config("rust-analyzer", {
--ELSE
-- Note the neovim name of the language server is rust_analyzer with an underscore.
vim.lsp.config("rust_analyzer", {
--ENDIF
    settings = {
        ["rust-analyzer"] = rust_analyzer,
    },
})
