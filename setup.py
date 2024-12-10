from setuptools import setup, find_packages

setup(
    name="mrt",
    version="0.1.0",
    packages=find_packages(),
    install_requires=[],
    python_requires=">=3.10",  # We use match statements which require Python 3.10+
)
