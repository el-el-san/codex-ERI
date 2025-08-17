#!/bin/bash

mkdir -p ~/release-tmp
cd ~/release-tmp

# Download artifacts
echo "Downloading Linux x86_64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781562155/zip > linux-x86_64.zip
unzip -q linux-x86_64.zip && rm linux-x86_64.zip

echo "Downloading Linux ARM64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781557225/zip > linux-aarch64.zip
unzip -q linux-aarch64.zip && rm linux-aarch64.zip

echo "Downloading Android ARM64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781557874/zip > android-aarch64.zip
unzip -q android-aarch64.zip && rm android-aarch64.zip

echo "Downloading Windows x86_64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781575452/zip > windows-x86_64.zip
unzip -q windows-x86_64.zip && rm windows-x86_64.zip

echo "Downloading Windows ARM64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781564392/zip > windows-aarch64.zip
unzip -q windows-aarch64.zip && rm windows-aarch64.zip

echo "Downloading macOS x86_64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781577250/zip > macos-x86_64.zip
unzip -q macos-x86_64.zip && rm macos-x86_64.zip

echo "Downloading macOS ARM64..."
gh api repos/el-el-san/codex-ERI/actions/artifacts/3781564458/zip > macos-aarch64.zip
unzip -q macos-aarch64.zip && rm macos-aarch64.zip

ls -la *.tar.gz *.zip 2>/dev/null