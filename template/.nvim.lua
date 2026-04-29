--INCLUDEFILE option("neovim")
-- You must enable the exrc setting in neovim for this config file to be used.
local rust_analyzer = {
    cargo = {
        --REPLACE riscv32imac-unknown-none-elf rust_target
        target = "riscv32imac-unknown-none-elf",
        allTargets = false,
        --IF option("xtensa")
        --REPLACE esp rust_toolchain
        extraEnv = { RUSTUP_TOOLCHAIN = "esp" },
        --ENDIF
    },
}

--IF option("neovim-rustaceanvim")
-- Note the rustaceanvim name of the language server is rust-analyzer with a dash.
vim.lsp.config("rust-analyzer", {
--ELSE
-- Note the neovim name of the language server is rust_analyzer with an underscore.
vim.lsp.config("rust_analyzer", {
--ENDIF
--IF option("xtensa")
	cmd = { "rustup", "run", "stable", "rust-analyzer" },
--ENDIF
    settings = {
        ["rust-analyzer"] = rust_analyzer,
    },
})
