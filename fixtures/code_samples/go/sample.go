package main

import "fmt"

// Config holds application configuration.
type Config struct {
	Name  string
	Value int64
}

// NewConfig creates a new Config.
func NewConfig(name string, value int64) *Config {
	return &Config{Name: name, Value: value}
}

// Display returns a formatted string.
func (c *Config) Display() string {
	return fmt.Sprintf("%s: %d", c.Name, c.Value)
}

func helper(x int) int {
	return x * 2
}

func main() {
	cfg := NewConfig("test", 42)
	fmt.Println(cfg.Display())
	fmt.Println(helper(10))
}
