<?php

class Config {
    public string $name;
    public int $value;

    public function __construct(string $name, int $value) {
        $this->name = $name;
        $this->value = $value;
    }

    public function display(): string {
        return "{$this->name}: {$this->value}";
    }
}

interface Processor {
    public function process(string $input): string;
    public function validate(): bool;
}

function helper(int $x): int {
    return $x * 2;
}

$cfg = new Config("test", 42);
echo $cfg->display();
echo helper(10);
